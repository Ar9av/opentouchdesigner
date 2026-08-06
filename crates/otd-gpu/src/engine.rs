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
use std::hash::{Hash, Hasher};

use otd_core::cook::resolve_bypass;
use otd_core::{CookContext, CookError, Cooker, EvalContext, Graph, NodeId};
use slotmap::SecondaryMap;
use wgpu::util::DeviceExt;

use crate::context::GpuContext;
use crate::ops::{self, PackedParams, Sizing};
use crate::shader;
use crate::texture::{TOP_FORMAT, TexturePool, TopTexture};

/// Resolution used by an operator with nothing to inherit from.
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
    /// Pipeline key of the last shader that compiled, for operators whose
    /// shader comes from a parameter. Kept so a typo mid-edit falls back to
    /// the last working shader instead of going black.
    shader_key: Option<String>,
    shader_error: Option<String>,
}

pub struct TopEngine {
    ctx: GpuContext,
    bind_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    sampler_linear: wgpu::Sampler,
    sampler_nearest: wgpu::Sampler,
    dummy: wgpu::TextureView,
    pipelines: HashMap<String, wgpu::RenderPipeline>,
    /// Shaders that failed to compile, so a broken source is not recompiled
    /// on every frame.
    failed: HashMap<String, String>,
    /// The fullscreen triangle compiled from GLSL, paired with GLSL fragment
    /// stages. Built on first use so a build with no GLSL shader never pays
    /// for it.
    glsl_vertex: Option<wgpu::ShaderModule>,
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

        let make_sampler = |filter, label| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: filter,
                min_filter: filter,
                mipmap_filter: wgpu::MipmapFilterMode::Nearest,
                ..Default::default()
            })
        };
        let sampler_linear = make_sampler(wgpu::FilterMode::Linear, "otd sampler linear");
        let sampler_nearest = make_sampler(wgpu::FilterMode::Nearest, "otd sampler nearest");

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
            sampler_linear,
            sampler_nearest,
            dummy,
            pipelines: HashMap::new(),
            failed: HashMap::new(),
            glsl_vertex: None,
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

    // ------------------------------------------------------------ pipelines

    fn build_pipeline(
        &self,
        label: &str,
        vertex: &wgpu::ShaderModule,
        fragment: &wgpu::ShaderModule,
        entry: &str,
    ) -> wgpu::RenderPipeline {
        // WGSL modules use our own names; a module compiled from GLSL has a
        // single entry point called `main` in both stages.
        let (vertex_entry, fragment_entry) = if entry == "main" {
            ("main", "main")
        } else {
            ("vs_main", entry)
        };
        self.ctx
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&self.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: vertex,
                    entry_point: Some(vertex_entry),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: fragment,
                    entry_point: Some(fragment_entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: TOP_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
    }

    /// Compile (once) and return the pipeline for a built-in operator.
    fn builtin_pipeline(&mut self, type_name: &str) -> Result<&wgpu::RenderPipeline, CookError> {
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
            let pipeline = self.build_pipeline(type_name, &module, &module, "fs_main");
            self.pipelines.insert(type_name.to_string(), pipeline);
        }
        Ok(&self.pipelines[type_name])
    }

    fn glsl_vertex_module(&mut self) -> wgpu::ShaderModule {
        if self.glsl_vertex.is_none() {
            self.glsl_vertex = Some(self.ctx.device.create_shader_module(
                wgpu::ShaderModuleDescriptor {
                    label: Some("otd glsl vertex"),
                    source: wgpu::ShaderSource::Glsl {
                        shader: shader::VERTEX_GLSL.into(),
                        stage: wgpu::naga::ShaderStage::Vertex,
                        defines: &[],
                    },
                },
            ));
        }
        self.glsl_vertex.clone().unwrap()
    }

    /// Compile a user shader. Returns the pipeline key, or the error to show
    /// on the node.
    ///
    /// Sources are validated with naga *before* wgpu sees them, so a typo
    /// produces a message with a line number rather than a device-lost.
    fn user_pipeline(&mut self, source: &str, is_glsl: bool) -> Result<String, String> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        source.hash(&mut hasher);
        is_glsl.hash(&mut hasher);
        let key = format!("user:{:016x}", hasher.finish());

        if self.pipelines.contains_key(&key) {
            return Ok(key);
        }
        if let Some(err) = self.failed.get(&key) {
            return Err(err.clone());
        }

        let result = if is_glsl {
            let full = shader::wrap_glsl(source);
            let vertex = self.glsl_vertex_module();
            shader::validate_glsl(&full).map(|()| {
                let module = self
                    .ctx
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some(&key),
                        source: wgpu::ShaderSource::Glsl {
                            shader: full.into(),
                            stage: wgpu::naga::ShaderStage::Fragment,
                            defines: &[],
                        },
                    });
                self.build_pipeline(&key, &vertex, &module, "main")
            })
        } else {
            let full = shader::wrap_wgsl(source);
            shader::validate_wgsl(&full).map(|()| {
                let module = self
                    .ctx
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some(&key),
                        source: wgpu::ShaderSource::Wgsl(full.into()),
                    });
                self.build_pipeline(&key, &module, &module, "fs_main")
            })
        };

        match result {
            Ok(pipeline) => {
                self.pipelines.insert(key.clone(), pipeline);
                Ok(key)
            }
            Err(e) => {
                self.failed.insert(key, e.clone());
                Err(e)
            }
        }
    }

    // ---------------------------------------------------------------- frame

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

    /// The compile error for a node's user shader, if it has one.
    pub fn shader_error(&self, id: NodeId) -> Option<&str> {
        self.nodes.get(id).and_then(|n| n.shader_error.as_deref())
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

    fn run_pass(&mut self, pass: Pass<'_>) -> Result<(), CookError> {
        // Pipeline lookup borrows self mutably; clone the handle out so the
        // bind group and the encoder can be touched afterwards.
        let pipeline = match pass.pipeline {
            PipelineRef::Builtin(name) => self.builtin_pipeline(name)?.clone(),
            PipelineRef::Key(key) => self
                .pipelines
                .get(key)
                .ok_or_else(|| CookError::op(pass.label, "shader is not compiled"))?
                .clone(),
        };
        let sampler = if pass.nearest {
            self.sampler_nearest.clone()
        } else {
            self.sampler_linear.clone()
        };
        let bind = self
            .ctx
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(pass.label),
                layout: &self.bind_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: pass.uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(pass.in0),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(pass.in1),
                    },
                ],
            });

        let encoder = self
            .encoder
            .as_mut()
            .ok_or_else(|| CookError::op(pass.label, "cook outside of begin_frame/end_frame"))?;
        {
            let mut render = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(pass.label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: pass.target,
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
            render.set_pipeline(&pipeline);
            render.set_bind_group(0, &bind, &[]);
            render.draw(0..3, 0..1);
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

    fn uniforms(
        &self,
        width: u32,
        height: u32,
        ctx: &CookContext,
        params: PackedParams,
    ) -> Uniforms {
        Uniforms {
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
        }
    }

    /// Copy another node's current texture into `id`'s output. Shared by
    /// Feedback (which reads before the target re-cooks) and Select (which
    /// reads after, because it declares the target as a dependency).
    fn blit_from(
        &mut self,
        graph: &Graph,
        id: NodeId,
        source: Option<NodeId>,
        ctx: &CookContext,
        label: &str,
    ) -> Result<(), CookError> {
        let src = source
            .filter(|s| *s != id)
            .and_then(|s| self.output(graph, s))
            .cloned();
        let Some(src) = src else {
            // Nothing to read yet: keep a black frame so downstream operators
            // still have something to sample.
            self.ensure_output(id, FALLBACK_SIZE.0, FALLBACK_SIZE.1);
            return Ok(());
        };
        self.ensure_output(id, src.key.width, src.key.height);
        let out = self.nodes[id].output.as_ref().unwrap().view.clone();
        let uniforms = self.uniforms(src.key.width, src.key.height, ctx, [[0.0; 4]; 4]);
        let buf = self.write_uniform(id, 0, uniforms);
        let dummy = self.dummy.clone();
        self.run_pass(Pass {
            label,
            pipeline: PipelineRef::Builtin(ops::NULL),
            target: &out,
            uniform: &buf,
            in0: &src.view,
            in1: &dummy,
            nearest: false,
        })
    }
}

enum PipelineRef<'a> {
    Builtin(&'a str),
    Key(&'a str),
}

struct Pass<'a> {
    label: &'a str,
    pipeline: PipelineRef<'a>,
    target: &'a wgpu::TextureView,
    uniform: &'a wgpu::Buffer,
    in0: &'a wgpu::TextureView,
    in1: &'a wgpu::TextureView,
    nearest: bool,
}

/// The TOP a parameter-named operator points at.
fn referenced_target(graph: &Graph, id: NodeId, key: &str) -> Option<NodeId> {
    let node = graph.get(id)?;
    let path = node.param(key)?.eval(&EvalContext::default()).as_str();
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    graph.find(path).filter(|t| *t != id)
}

impl TopEngine {
    /// Cook one TOP with an explicit evaluation context.
    ///
    /// The context is passed in rather than derived from `ctx` because a
    /// parameter in Export mode has to resolve against the CHOP channels
    /// cooked this frame, and only the cross-family router knows about those.
    pub fn cook_node(
        &mut self,
        graph: &Graph,
        id: NodeId,
        ctx: &CookContext,
        eval: &EvalContext,
    ) -> Result<(), CookError> {
        self.cook_top(graph, id, ctx, eval)
    }
}

impl Cooker for TopEngine {
    fn extra_inputs(&self, graph: &Graph, id: NodeId) -> Vec<NodeId> {
        let Some(node) = graph.get(id) else {
            return Vec::new();
        };
        // Select reads the current frame, so its target must cook first.
        // Feedback deliberately does not appear here.
        if node.op_type != ops::SELECT {
            return Vec::new();
        }
        referenced_target(graph, id, "top").into_iter().collect()
    }

    fn cook(&mut self, graph: &Graph, id: NodeId, ctx: &CookContext) -> Result<(), CookError> {
        // No CHOPs in sight: parameters resolve against time alone.
        let eval = ctx.eval_ctx();
        self.cook_top(graph, id, ctx, &eval)
    }
}

impl TopEngine {
    fn cook_top(
        &mut self,
        graph: &Graph,
        id: NodeId,
        ctx: &CookContext,
        eval: &EvalContext,
    ) -> Result<(), CookError> {
        let node = graph.get(id).ok_or(CookError::NoSuchNode)?;
        let path = graph.path(id);

        // COMPs hold sub-networks; they have no texture of their own yet.
        if node.family != otd_core::Family::Top {
            return Ok(());
        }

        let spec = ops::spec(&node.op_type)
            .ok_or_else(|| CookError::op(&path, format!("unknown TOP `{}`", node.op_type)))?;
        // ---- Feedback reads its target as it stands right now, which is last
        // frame's content because the target has not re-cooked yet.
        if node.op_type == ops::FEEDBACK {
            let target = referenced_target(graph, id, "target");
            return self.blit_from(graph, id, target, ctx, &path);
        }
        // ---- Select reads the same way, but its target has already cooked.
        if node.op_type == ops::SELECT {
            let target = referenced_target(graph, id, "top");
            return self.blit_from(graph, id, target, ctx, &path);
        }
        // ---- Cache holds whatever it last saw while Active is off.
        if node.op_type == ops::CACHE {
            let active = node
                .param("active")
                .map(|p| p.eval(eval).as_bool())
                .unwrap_or(true);
            if !active && self.nodes.get(id).is_some_and(|n| n.output.is_some()) {
                return Ok(());
            }
        }

        // ---- Resolution
        let (width, height) = match spec.sizing {
            Sizing::Params => ops::generator_size(node, eval),
            Sizing::Input0 => self
                .input_size(graph, id, 0)
                .or_else(|| self.input_size(graph, id, 1))
                .unwrap_or(FALLBACK_SIZE),
            Sizing::Input0OrParams => self
                .input_size(graph, id, 0)
                .unwrap_or_else(|| ops::generator_size(node, eval)),
            // Handled above; a Referenced operator never reaches here.
            Sizing::Referenced => FALLBACK_SIZE,
        };
        self.ensure_output(id, width, height);

        // ---- Shader: built in, or compiled from a parameter.
        let mut pipeline_key: Option<String> = None;
        if spec.dynamic_shader {
            let (source, is_glsl) = ops::shader_source(node, eval);
            match self.user_pipeline(&source, is_glsl) {
                Ok(key) => {
                    let entry = self.nodes.entry(id).unwrap().or_default();
                    entry.shader_error = None;
                    entry.shader_key = Some(key.clone());
                    pipeline_key = Some(key);
                }
                Err(e) => {
                    let entry = self.nodes.entry(id).unwrap().or_default();
                    entry.shader_error = Some(e);
                    // Hold the last shader that worked, so an edit in progress
                    // does not black out a running patch.
                    pipeline_key = entry.shader_key.clone();
                    if pipeline_key.is_none() {
                        return Ok(());
                    }
                }
            }
        }

        let params = (spec.pack)(node, eval);
        let base = self.uniforms(width, height, ctx, params);
        let nearest = node
            .param("filter")
            .map(|p| p.eval(eval).as_str() == "nearest")
            .unwrap_or(false);

        let in0 = self.input_view(graph, id, 0);
        let in1 = self.input_view(graph, id, 1);
        let out = self.nodes[id].output.as_ref().unwrap().view.clone();
        let type_name = spec.def.type_name;

        if spec.two_pass {
            // Horizontal into scratch, vertical into the output.
            let scratch = self.pool.acquire(&self.ctx.device, width, height);
            let scratch_view = scratch.view.clone();
            self.scratch.push(scratch);
            let dummy = self.dummy.clone();

            let mut first = base;
            first.params[0][1] = 0.0;
            let buf0 = self.write_uniform(id, 0, first);
            self.run_pass(Pass {
                label: &path,
                pipeline: PipelineRef::Builtin(type_name),
                target: &scratch_view,
                uniform: &buf0,
                in0: &in0,
                in1: &dummy,
                nearest,
            })?;

            let mut second = base;
            second.params[0][1] = 1.0;
            let buf1 = self.write_uniform(id, 1, second);
            self.run_pass(Pass {
                label: &path,
                pipeline: PipelineRef::Builtin(type_name),
                target: &out,
                uniform: &buf1,
                in0: &scratch_view,
                in1: &dummy,
                nearest,
            })
        } else {
            let buf = self.write_uniform(id, 0, base);
            let pipeline = match &pipeline_key {
                Some(key) => PipelineRef::Key(key.as_str()),
                None => PipelineRef::Builtin(type_name),
            };
            self.run_pass(Pass {
                label: &path,
                pipeline,
                target: &out,
                uniform: &buf,
                in0: &in0,
                in1: &in1,
                nearest,
            })
        }
    }
}
