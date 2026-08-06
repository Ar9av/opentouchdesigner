//! The TOP cook backend — the frame-graph half of PLAN.md §4.
//!
//! `otd-core`'s [`CookEngine`] decides *what* runs; this decides *how*. It
//! implements [`Cooker`], so from core's point of view producing a texture is
//! an opaque side effect.
//!
//! One command encoder is recorded per frame and submitted once. Recording in
//! cook order is what makes the Feedback TOP work: it blits its target's
//! texture at the point in the command stream *before* the target's own pass
//! rewrites it, so it sees last frame's content without any explicit
//! double-buffering.

use std::collections::HashMap;

use otd_core::cook::resolve_bypass;
use otd_core::{CookContext, CookError, Cooker, Graph, NodeId};
use slotmap::SecondaryMap;
use wgpu::util::DeviceExt;

use crate::context::GpuContext;
use crate::ops::{self, PackedParams};
use crate::texture::{TOP_FORMAT, TexturePool, TopTexture};

/// Resolution used by a filter whose input has not produced anything.
pub const FALLBACK_SIZE: (u32, u32) = (1280, 720);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    res: [f32; 4],
    time: [f32; 4],
    params: PackedParams,
}

#[derive(Default)]
struct NodeGpu {
    output: Option<TopTexture>,
    /// One uniform buffer per pass (separable filters use two).
    uniforms: Vec<wgpu::Buffer>,
}

pub struct TopEngine {
    ctx: GpuContext,
    bind_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    sampler: wgpu::Sampler,
    dummy: wgpu::TextureView,
    pipelines: HashMap<&'static str, wgpu::RenderPipeline>,
    nodes: SecondaryMap<NodeId, NodeGpu>,
    pool: TexturePool,
    encoder: Option<wgpu::CommandEncoder>,
    /// Scratch targets used by two-pass operators, returned to the pool at
    /// the end of the frame.
    scratch: Vec<TopTexture>,
    pub passes_this_frame: u32,
}

impl TopEngine {
    pub fn new(ctx: GpuContext) -> Self {
        let device = &ctx.device;

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("otd top bind layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("otd top pipeline layout"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("otd top sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        // A 1x1 transparent texture for unconnected inputs, so every operator
        // can be compiled against the same bind group layout.
        let dummy_tex = device.create_texture_with_data(
            &ctx.queue,
            &wgpu::TextureDescriptor {
                label: Some("otd dummy input"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: TOP_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            // Rgba16Float zero — 8 bytes of zeroes is transparent black.
            &[0u8; 8],
        );
        let dummy = dummy_tex.create_view(&wgpu::TextureViewDescriptor::default());

        TopEngine {
            ctx,
            bind_layout,
            pipeline_layout,
            sampler,
            dummy,
            pipelines: HashMap::new(),
            nodes: SecondaryMap::new(),
            pool: TexturePool::new(),
            encoder: None,
            scratch: Vec::new(),
            passes_this_frame: 0,
        }
    }

    pub fn context(&self) -> &GpuContext {
        &self.ctx
    }

    /// Compile (once) and return the pipeline for an operator type.
    fn pipeline(&mut self, type_name: &'static str) -> Result<&wgpu::RenderPipeline, CookError> {
        if !self.pipelines.contains_key(type_name) {
            let spec = ops::spec(type_name)
                .ok_or_else(|| CookError::op(type_name, "no shader for this operator"))?;
            let source = format!("{}\n{}", ops::COMMON_WGSL, spec.shader);
            let module = self
                .ctx
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(type_name),
                    source: wgpu::ShaderSource::Wgsl(source.into()),
                });
            let pipeline =
                self.ctx
                    .device
                    .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some(type_name),
                        layout: Some(&self.pipeline_layout),
                        vertex: wgpu::VertexState {
                            module: &module,
                            entry_point: Some("vs_main"),
                            compilation_options: Default::default(),
                            buffers: &[],
                        },
                        primitive: wgpu::PrimitiveState::default(),
                        depth_stencil: None,
                        multisample: wgpu::MultisampleState::default(),
                        fragment: Some(wgpu::FragmentState {
                            module: &module,
                            entry_point: Some("fs_main"),
                            compilation_options: Default::default(),
                            targets: &[Some(wgpu::ColorTargetState {
                                format: TOP_FORMAT,
                                blend: None,
                                write_mask: wgpu::ColorWrites::ALL,
                            })],
                        }),
                        multiview_mask: None,
                        cache: None,
                    });
            self.pipelines.insert(type_name, pipeline);
        }
        Ok(&self.pipelines[type_name])
    }

    /// Start recording a frame. Must be paired with [`TopEngine::end_frame`].
    pub fn begin_frame(&mut self) {
        self.passes_this_frame = 0;
        self.encoder = Some(self.ctx.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor {
                label: Some("otd frame"),
            },
        ));
    }

    /// Submit the frame's work and recycle scratch targets.
    pub fn end_frame(&mut self) {
        if let Some(encoder) = self.encoder.take() {
            self.ctx.queue.submit(Some(encoder.finish()));
        }
        for t in std::mem::take(&mut self.scratch) {
            self.pool.release(t);
        }
    }

    /// The texture a node currently presents, following bypass flags.
    pub fn output(&self, graph: &Graph, id: NodeId) -> Option<&TopTexture> {
        let real = resolve_bypass(graph, id)?;
        self.nodes.get(real).and_then(|n| n.output.as_ref())
    }

    /// Drop a node's GPU resources, returning its texture to the pool.
    pub fn forget(&mut self, id: NodeId) {
        if let Some(mut n) = self.nodes.remove(id) {
            if let Some(t) = n.output.take() {
                self.pool.release(t);
            }
        }
    }

    /// Drop every node's resources. Called when a project is loaded.
    pub fn reset(&mut self) {
        let ids: Vec<NodeId> = self.nodes.keys().collect();
        for id in ids {
            self.forget(id);
        }
        self.pool.clear();
    }

    pub fn pooled_bytes(&self) -> u64 {
        self.pool.pooled_bytes()
    }

    pub fn textures_created(&self) -> u64 {
        self.pool.created
    }

    /// Total bytes held by live node outputs.
    pub fn resident_bytes(&self) -> u64 {
        self.nodes
            .values()
            .filter_map(|n| n.output.as_ref())
            .map(|t| t.key.width as u64 * t.key.height as u64 * 8)
            .sum()
    }

    fn ensure_output(&mut self, id: NodeId, width: u32, height: u32) {
        let entry = self.nodes.entry(id).unwrap().or_default();
        let matches = entry
            .output
            .as_ref()
            .map(|t| t.key.width == width && t.key.height == height)
            .unwrap_or(false);
        if matches {
            return;
        }
        let old = entry.output.take();
        let fresh = self.pool.acquire(&self.ctx.device, width, height);
        self.nodes[id].output = Some(fresh);
        if let Some(old) = old {
            self.pool.release(old);
        }
    }

    fn write_uniform(&mut self, id: NodeId, slot: usize, data: Uniforms) -> wgpu::Buffer {
        self.nodes.entry(id).unwrap().or_default();
        while self.nodes[id].uniforms.len() <= slot {
            let buf = self.ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("otd top uniforms"),
                size: std::mem::size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.nodes[id].uniforms.push(buf);
        }
        let buf = self.nodes[id].uniforms[slot].clone();
        self.ctx
            .queue
            .write_buffer(&buf, 0, bytemuck::bytes_of(&data));
        buf
    }

    #[allow(clippy::too_many_arguments)]
    fn run_pass(
        &mut self,
        label: &str,
        pipeline_key: &'static str,
        target: &wgpu::TextureView,
        uniform: &wgpu::Buffer,
        in0: &wgpu::TextureView,
        in1: &wgpu::TextureView,
    ) -> Result<(), CookError> {
        // `pipeline()` borrows self mutably; clone the handle out so the bind
        // group and the encoder can be touched afterwards.
        let pipeline = self.pipeline(pipeline_key)?.clone();
        let bind = self
            .ctx
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &self.bind_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(in0),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(in1),
                    },
                ],
            });

        let encoder = self
            .encoder
            .as_mut()
            .ok_or_else(|| CookError::op(label, "cook outside of begin_frame/end_frame"))?;
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        self.passes_this_frame += 1;
        Ok(())
    }

    fn input_view(&self, graph: &Graph, node_id: NodeId, index: usize) -> wgpu::TextureView {
        graph
            .get(node_id)
            .and_then(|n| n.inputs.get(index).copied().flatten())
            .and_then(|src| self.output(graph, src))
            .map(|t| t.view.clone())
            .unwrap_or_else(|| self.dummy.clone())
    }

    fn input_size(&self, graph: &Graph, node_id: NodeId, index: usize) -> Option<(u32, u32)> {
        let src = graph.get(node_id)?.inputs.get(index).copied().flatten()?;
        let t = self.output(graph, src)?;
        Some((t.key.width, t.key.height))
    }
}

impl Cooker for TopEngine {
    fn cook(&mut self, graph: &Graph, id: NodeId, ctx: &CookContext) -> Result<(), CookError> {
        let node = graph.get(id).ok_or(CookError::NoSuchNode)?;
        let path = graph.path(id);

        // COMPs hold sub-networks; they have no texture of their own yet.
        if node.family != otd_core::Family::Top {
            return Ok(());
        }

        let spec = ops::spec(&node.op_type)
            .ok_or_else(|| CookError::op(&path, format!("unknown TOP `{}`", node.op_type)))?;
        let eval = ctx.eval_ctx();

        // ---- Feedback: blit the target's texture as it stands right now,
        // which is last frame's content because the target has not re-cooked.
        if node.op_type == ops::FEEDBACK {
            let target_path = node
                .param("target")
                .map(|p| p.eval(&eval).as_str())
                .unwrap_or_default();
            let target = if target_path.trim().is_empty() {
                None
            } else {
                graph.find(target_path.trim())
            };
            let Some(target) = target.filter(|t| *t != id) else {
                // Nothing to read yet: keep a black frame so downstream
                // operators still have something to sample.
                self.ensure_output(id, FALLBACK_SIZE.0, FALLBACK_SIZE.1);
                return Ok(());
            };
            let Some(src) = self.output(graph, target).cloned() else {
                self.ensure_output(id, FALLBACK_SIZE.0, FALLBACK_SIZE.1);
                return Ok(());
            };
            self.ensure_output(id, src.key.width, src.key.height);
            let out = self.nodes[id].output.as_ref().unwrap().view.clone();
            let uniforms = Uniforms {
                res: [
                    src.key.width as f32,
                    src.key.height as f32,
                    1.0 / src.key.width as f32,
                    1.0 / src.key.height as f32,
                ],
                time: [
                    ctx.abs_time as f32,
                    ctx.time as f32,
                    ctx.frame as f32,
                    ctx.fps as f32,
                ],
                params: [[0.0; 4]; 4],
            };
            let buf = self.write_uniform(id, 0, uniforms);
            let dummy = self.dummy.clone();
            return self.run_pass(&path, ops::NULL, &out, &buf, &src.view, &dummy);
        }

        // ---- Resolution: generators declare it, filters inherit input 0.
        let (width, height) = if spec.generator {
            ops::generator_size(node, &eval)
        } else {
            self.input_size(graph, id, 0)
                .or_else(|| self.input_size(graph, id, 1))
                .unwrap_or(FALLBACK_SIZE)
        };
        self.ensure_output(id, width, height);

        let params = (spec.pack)(node, &eval);
        let base = Uniforms {
            res: [
                width as f32,
                height as f32,
                1.0 / width as f32,
                1.0 / height as f32,
            ],
            time: [
                ctx.abs_time as f32,
                ctx.time as f32,
                ctx.frame as f32,
                ctx.fps as f32,
            ],
            params,
        };

        let in0 = self.input_view(graph, id, 0);
        let in1 = self.input_view(graph, id, 1);
        let out = self.nodes[id].output.as_ref().unwrap().view.clone();
        let type_name = spec.def.type_name;

        if spec.two_pass {
            // Horizontal into scratch, vertical into the output.
            let scratch = self.pool.acquire(&self.ctx.device, width, height);
            let scratch_view = scratch.view.clone();
            self.scratch.push(scratch);

            let mut first = base;
            first.params[0][1] = 0.0;
            let buf0 = self.write_uniform(id, 0, first);
            let dummy = self.dummy.clone();
            self.run_pass(&path, type_name, &scratch_view, &buf0, &in0, &dummy)?;

            let mut second = base;
            second.params[0][1] = 1.0;
            let buf1 = self.write_uniform(id, 1, second);
            self.run_pass(&path, type_name, &out, &buf1, &scratch_view, &dummy)?;
            Ok(())
        } else {
            let buf = self.write_uniform(id, 0, base);
            self.run_pass(&path, type_name, &out, &buf, &in0, &in1)
        }
    }
}
