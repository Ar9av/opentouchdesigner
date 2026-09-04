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
use otd_core::{CookContext, CookError, Cooker, Crossings, EvalContext, Graph, NodeId};
use slotmap::SecondaryMap;
use wgpu::util::DeviceExt;

use crate::context::GpuContext;
use crate::ops::{self, PackedParams, Sizing};
use crate::record::Recorder;
use crate::render3d::{self, Renderer};
use crate::scene::Scene;
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
    /// Depth buffer for a Render TOP, matched to its colour target.
    depth: Option<(u32, u32, wgpu::TextureView)>,
    /// One uniform buffer per pass (separable filters use two).
    uniforms: Vec<wgpu::Buffer>,
    /// Pipeline key of the last shader that compiled, for operators whose
    /// shader comes from a parameter. Kept so a typo mid-edit falls back to
    /// the last working shader instead of going black.
    shader_key: Option<String>,
    shader_error: Option<String>,
    /// A message from something that is not a shader — a missing movie file,
    /// a camera that would not open. Shown on the node body.
    status: Option<String>,
    /// The RGBA8 texture a decoded video frame is uploaded into, and the
    /// decoder revision it holds, so a frame is uploaded once rather than
    /// every time the node cooks.
    upload: Option<Upload>,
}

/// A CPU-written texture: where a decoded frame lands before the GPU sees it.
struct Upload {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    revision: u64,
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
    /// One decoder per Movie File In / Video Device In node. Held here
    /// rather than in `NodeGpu` because dropping one kills a thread and a
    /// subprocess, and that must happen exactly when the node goes away.
    videos: SecondaryMap<NodeId, crate::video::Source>,
    pool: TexturePool,
    encoder: Option<wgpu::CommandEncoder>,
    /// Scratch targets used by two-pass operators, returned to the pool at
    /// the end of the frame.
    scratch: Vec<TopTexture>,
    /// The 3D pipeline, built on first use so a 2D-only patch never pays for
    /// it.
    renderer: Option<Renderer>,
    /// One encoder subprocess per recording Movie File Out node. Held here
    /// rather than in `NodeGpu` for the reason the video decoders are:
    /// dropping one finalises a file on disk, and that must happen exactly
    /// when the node stops recording, not whenever a texture is recycled.
    recorders: SecondaryMap<NodeId, Recorder>,
    /// Nodes that asked to be recorded this frame, drained in `end_frame`
    /// once the encoder has been submitted and the pixels are real.
    pending_records: Vec<NodeId>,
    /// Parsed font faces, shared by every Text TOP.
    fonts: crate::text::FontCache,
    /// What each Text TOP last rasterised, so retyping one caption does not
    /// re-lay-out every other one — and so a static caption costs nothing at
    /// all after its first frame.
    text_keys: SecondaryMap<NodeId, String>,
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
            videos: SecondaryMap::new(),
            pool: TexturePool::new(),
            encoder: None,
            scratch: Vec::new(),
            renderer: None,
            recorders: SecondaryMap::new(),
            pending_records: Vec::new(),
            fonts: crate::text::FontCache::default(),
            text_keys: SecondaryMap::new(),
            passes_this_frame: 0,
        }
    }

    /// Draw calls and instances issued this frame, for the perf panel.
    pub fn render_stats(&self) -> (u32, u32) {
        self.renderer
            .as_ref()
            .map(|r| (r.draws_this_frame, r.instances_this_frame))
            .unwrap_or((0, 0))
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
        if let Some(r) = self.renderer.as_mut() {
            r.begin_frame();
        }
        self.encoder = Some(self.ctx.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor {
                label: Some("otd frame"),
            },
        ));
    }

    /// Submit the frame's work, capture any recordings, and recycle scratch.
    pub fn end_frame(&mut self) {
        if let Some(encoder) = self.encoder.take() {
            self.ctx.queue.submit(Some(encoder.finish()));
        }
        // *After* the submit, on purpose: a readback taken during the cook
        // copies a texture whose passes have not run yet, and a recording one
        // frame behind what the artist watched is a file that is wrong.
        for id in std::mem::take(&mut self.pending_records) {
            self.capture(id);
        }
        for t in std::mem::take(&mut self.scratch) {
            self.pool.release(t);
        }
    }

    /// Copy a recording node's output to its encoder.
    fn capture(&mut self, id: NodeId) {
        let Some(tex) = self.nodes.get(id).and_then(|n| n.output.as_ref()) else {
            return;
        };
        let Some(recorder) = self.recorders.get(id) else {
            return;
        };
        if tex.key.width != recorder.width || tex.key.height != recorder.height {
            // The input changed size mid-recording. Every frame in a file has
            // to be the same shape, so this is a new file rather than a
            // silently stretched one; the next cook notices the key changed.
            return;
        }
        match crate::read_pixels_rgba8(&self.ctx, tex) {
            Ok((_, _, pixels)) => recorder.push(pixels),
            Err(e) => {
                self.nodes.entry(id).unwrap().or_default().status =
                    Some(format!("could not read the frame back: {e}"));
            }
        }
    }

    /// Lay out and upload a Text TOP's glyphs, then tint them.
    #[allow(clippy::too_many_arguments)]
    fn cook_text(
        &mut self,
        id: NodeId,
        ctx: &CookContext,
        eval: &EvalContext,
        node: &otd_core::Node,
        width: u32,
        height: u32,
        label: &str,
    ) -> Result<(), CookError> {
        let text = |key: &str| {
            node.param(key)
                .map(|p| p.eval(eval).as_str())
                .unwrap_or_default()
        };
        let number = |key: &str, fallback: f32| {
            node.param(key)
                .map(|p| p.eval(eval).as_f32())
                .unwrap_or(fallback)
        };
        let index = |key: &str| {
            node.param(key)
                .and_then(|p| {
                    let chosen = p.eval(eval).as_str();
                    p.menu.as_ref()?.iter().position(|i| *i == chosen)
                })
                .unwrap_or(0)
        };

        let font_path = text("font");
        let layout = crate::text::Layout {
            text: text("text"),
            size: number("size", 48.0).max(1.0),
            line_spacing: number("linespacing", 1.2),
            horizontal: crate::text::Align::from_index(index("halign")),
            vertical: crate::text::Align::from_index(index("valign")),
            width,
            height,
            wrap: node
                .param("wrap")
                .map(|p| p.eval(eval).as_bool())
                .unwrap_or(true),
        };

        // Everything the raster depends on. The colour is deliberately absent:
        // it is applied in the shader, so changing it must not re-rasterise.
        let key = format!(
            "{font_path}|{}|{}|{}|{}|{}|{width}x{height}|{}",
            layout.text,
            layout.size,
            layout.line_spacing,
            index("halign"),
            index("valign"),
            layout.wrap
        );

        if self.text_keys.get(id).map(|k| k.as_str()) != Some(key.as_str()) {
            match self.fonts.get(&font_path) {
                Ok(font) => {
                    let raster = crate::text::rasterise(font, &layout);
                    self.upload_pixels(
                        id,
                        raster.width,
                        raster.height,
                        wgpu::TextureFormat::Rgba8Unorm,
                        &raster.pixels,
                        u64::MAX,
                    );
                    self.text_keys.insert(id, key);
                    self.nodes.entry(id).unwrap().or_default().status = None;
                }
                Err(e) => {
                    // No font is a message on the node and a blank picture, not
                    // a failed cook: losing the whole render because a caption
                    // cannot find a face is the wrong trade.
                    self.nodes.entry(id).unwrap().or_default().status = Some(e);
                    self.text_keys.remove(id);
                    return self.clear_to_black(id, ctx, label);
                }
            }
        }

        let Some(source_view) = self.nodes[id].upload.as_ref().map(|u| u.view.clone()) else {
            return self.clear_to_black(id, ctx, label);
        };
        let params = (ops::spec(&node.op_type).unwrap().pack)(node, eval);
        let out = self.nodes[id].output.as_ref().unwrap().view.clone();
        let uniforms = self.uniforms(width, height, ctx, params);
        let buf = self.write_uniform(id, 0, uniforms);
        let dummy = self.dummy.clone();
        self.run_pass(Pass {
            label,
            pipeline: PipelineRef::Builtin(ops::TEXT),
            target: &out,
            uniform: &buf,
            in0: &source_view,
            in1: &dummy,
            nearest: false,
        })
    }

    /// Start, stop or keep a recording, and note whether to capture this frame.
    fn cook_movie_out(&mut self, id: NodeId, node: &otd_core::Node, eval: &EvalContext) {
        let recording = node
            .param("record")
            .map(|p| p.eval(eval).as_bool())
            .unwrap_or(false);
        if !recording {
            // Dropping the recorder is what closes the file — see its `Drop`.
            if self.recorders.remove(id).is_some() {
                self.nodes.entry(id).unwrap().or_default().status = None;
            }
            return;
        }

        let Some((width, height)) = self
            .nodes
            .get(id)
            .and_then(|n| n.output.as_ref().map(|t| (t.key.width, t.key.height)))
        else {
            return;
        };
        let text = |key: &str| {
            node.param(key)
                .map(|p| p.eval(eval).as_str())
                .unwrap_or_default()
        };
        let path = text("file");
        let fps = node
            .param("fps")
            .map(|p| p.eval(eval).as_f32() as f64)
            .unwrap_or(60.0);
        let codec = match text("codec").as_str() {
            "h265" => "libx265",
            "prores" => "prores_ks",
            _ => "libx264",
        };
        let quality = node
            .param("quality")
            .map(|p| p.eval(eval).as_i64())
            .unwrap_or(75)
            .clamp(0, 100) as u32;

        // Everything that would make the frames inconsistent is in the key, so
        // changing any of it starts a new file instead of writing mismatched
        // frames into the open one.
        let key = format!("{path}|{width}x{height}|{fps:.3}|{codec}|{quality}");
        if self.recorders.get(id).map(|r| r.key.as_str()) != Some(key.as_str()) {
            self.recorders.remove(id);
            match Recorder::start(key, &path, width, height, fps, quality, codec) {
                Ok(r) => {
                    self.recorders.insert(id, r);
                    self.nodes.entry(id).unwrap().or_default().status = None;
                }
                Err(e) => {
                    self.nodes.entry(id).unwrap().or_default().status = Some(e);
                    return;
                }
            }
        }

        if let Some(problem) = self.recorders.get(id).and_then(|r| r.problem()) {
            self.nodes.entry(id).unwrap().or_default().status = Some(problem);
        } else {
            let written = self
                .recorders
                .get(id)
                .map(|r| r.frames_written())
                .unwrap_or(0);
            self.nodes.entry(id).unwrap().or_default().status =
                Some(format!("recording — {written} frames"));
        }
        self.pending_records.push(id);
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

    /// A non-shader message — a missing movie file, a camera that would not
    /// open — for display on the node body.
    pub fn status(&self, id: NodeId) -> Option<&str> {
        self.nodes.get(id).and_then(|n| n.status.as_deref())
    }

    /// Drop a node's GPU resources, returning its texture to the pool.
    ///
    /// Dropping the decoder is what stops its thread and kills its ffmpeg
    /// process, so deleting a camera node releases the camera.
    /// How many frames a recording node has written, for the editor.
    pub fn frames_recorded(&self, id: NodeId) -> Option<u64> {
        self.recorders.get(id).map(|r| r.frames_written())
    }

    pub fn forget(&mut self, id: NodeId) {
        self.videos.remove(id);
        // Deleting a recording node finalises its file rather than truncating
        // it: the drop closes the pipe and waits for ffmpeg's trailer.
        self.recorders.remove(id);
        self.pending_records.retain(|p| *p != id);
        self.text_keys.remove(id);
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

    /// Draw the scene a Render TOP names.
    #[allow(clippy::too_many_arguments)]
    fn cook_render(
        &mut self,
        graph: &Graph,
        id: NodeId,
        _ctx: &CookContext,
        eval: &EvalContext,
        scene: Option<&dyn Scene>,
        width: u32,
        height: u32,
    ) -> Result<(), CookError> {
        let Some(scene) = scene else {
            // A pure-TOP host has no geometry to give us; leave the last
            // frame rather than clearing to black.
            return Ok(());
        };
        let node = graph.get(id).ok_or(CookError::NoSuchNode)?;
        let description = render3d::describe(
            graph,
            node,
            eval,
            scene,
            width as f32 / height.max(1) as f32,
        );

        // A depth buffer matched to the colour target, rebuilt only on resize.
        let entry = self.nodes.entry(id).unwrap().or_default();
        let needs_depth = entry
            .depth
            .as_ref()
            .map(|(w, h, _)| *w != width || *h != height)
            .unwrap_or(true);
        if needs_depth {
            let view = render3d::depth_texture(&self.ctx.device, width, height);
            self.nodes[id].depth = Some((width, height, view));
        }

        let target = self.nodes[id].output.as_ref().unwrap().view.clone();
        let depth = self.nodes[id].depth.as_ref().unwrap().2.clone();
        if self.renderer.is_none() {
            self.renderer = Some(Renderer::new(&self.ctx.device));
        }

        // Colour maps are looked up before the borrow of the renderer.
        let maps: HashMap<NodeId, wgpu::TextureView> = description
            .items
            .iter()
            .filter_map(|i| i.color_map)
            .filter_map(|m| self.output(graph, m).map(|t| (m, t.view.clone())))
            .collect();
        let dummy = self.dummy.clone();
        let device = self.ctx.device.clone();

        let TopEngine {
            renderer, encoder, ..
        } = self;
        let renderer = renderer.as_mut().unwrap();
        let encoder = encoder
            .as_mut()
            .ok_or_else(|| CookError::op("renderTOP", "cook outside of a frame"))?;
        renderer.draw(
            &device,
            encoder,
            &target,
            &depth,
            &description,
            &dummy,
            &|id| maps.get(&id).cloned(),
        );
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

    /// The texture a node presents to the world. `output` already resolves
    /// bypass flags and component boundaries, so a component reads as its Out
    /// operator's texture with no special case at the call site.
    pub fn presented(&self, graph: &Graph, id: NodeId) -> Option<&TopTexture> {
        self.output(graph, id)
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
    /// Cook a Movie File In or a Video Device In.
    ///
    /// The shape is the same as every device operator in this codebase: open
    /// on demand, keyed by what the parameters ask for; take whatever the
    /// worker thread has produced; and turn a failure into a message on the
    /// node rather than into a failed cook.
    fn cook_video(
        &mut self,
        graph: &Graph,
        id: NodeId,
        ctx: &CookContext,
        eval: &EvalContext,
        label: &str,
    ) -> Result<(), CookError> {
        let node = graph.get(id).ok_or(CookError::NoSuchNode)?;
        let camera = node.op_type == ops::VIDEO_DEVICE_IN;
        let get = |key: &str| node.param(key).map(|p| p.eval(eval));
        let fallback = ops::generator_size(node, eval);

        // ---- What is being asked for, as one string. A change to it is
        // what reopens the source, so nothing else has to track staleness.
        let (key, file) = if camera {
            let device = get("device").map(|v| v.as_str()).unwrap_or_default();
            let fps = get("fps")
                .map(|v| v.as_f64())
                .unwrap_or(30.0)
                .clamp(1.0, 240.0);
            (
                format!("camera:{device}:{}x{}:{fps}", fallback.0, fallback.1),
                device,
            )
        } else {
            let file = get("file").map(|v| v.as_str()).unwrap_or_default();
            let resolved = if file.trim().is_empty() {
                String::new()
            } else {
                graph.resolve_external(file.trim()).display().to_string()
            };
            (format!("file:{resolved}"), resolved)
        };

        let active = get("active").map(|v| v.as_bool()).unwrap_or(true);
        if !active || file.trim().is_empty() {
            // Nothing asked for: release the device and show black at the
            // fallback size, so a downstream chain still has a shape.
            self.videos.remove(id);
            let entry = self.nodes.entry(id).unwrap().or_default();
            entry.status = None;
            self.ensure_output(id, fallback.0, fallback.1);
            return self.clear_to_black(id, ctx, label);
        }

        // ---- Open, if this is a different source from the one running.
        if self.videos.get(id).map(|s| s.key.as_str()) != Some(key.as_str()) {
            self.videos.remove(id);
            let opened = if camera {
                let fps = get("fps")
                    .map(|v| v.as_f64())
                    .unwrap_or(30.0)
                    .clamp(1.0, 240.0);
                crate::video::Source::camera(key.clone(), &file, fallback.0, fallback.1, fps)
            } else if crate::video::is_still(&file) {
                Ok(crate::video::Source::still(
                    key.clone(),
                    std::path::Path::new(&file),
                ))
            } else {
                match crate::video::probe(&file) {
                    Some(info) => crate::video::Source::file(key.clone(), &file, info, 0),
                    None if !std::path::Path::new(&file).exists() => {
                        Err(format!("no file at {file}"))
                    }
                    None => Err(format!(
                        "could not read {file} — ffmpeg/ffprobe not installed, \
                         or not a video this build can decode"
                    )),
                }
            };
            let entry = self.nodes.entry(id).unwrap().or_default();
            match opened {
                Ok(source) => {
                    entry.status = None;
                    self.videos.insert(id, source);
                }
                Err(e) => {
                    entry.status = Some(e);
                    self.ensure_output(id, fallback.0, fallback.1);
                    return self.clear_to_black(id, ctx, label);
                }
            }
        }

        // ---- Which frame the timeline is asking for. Playback is a
        // function of the timeline, not a private play head, so scrubbing
        // scrubs the picture and a headless render reads the same frames the
        // editor showed.
        let (wanted, ended_message) = {
            let source = &self.videos[id];
            if camera || source.info.fps <= 0.0 {
                (0, false)
            } else {
                let speed = get("speed").map(|v| v.as_f64()).unwrap_or(1.0);
                let raw = (ctx.time * speed * source.info.fps).floor() as i64;
                let length = (source.info.duration * source.info.fps).round() as i64;
                let mode = get("play").map(|v| v.as_str()).unwrap_or_default();
                match mode.as_str() {
                    "loop" if length > 1 => (raw.rem_euclid(length), false),
                    "hold" if length > 1 => (raw.clamp(0, length - 1), false),
                    "once" if length > 1 => (raw.clamp(0, length - 1), raw >= length),
                    _ => (raw.max(0), false),
                }
            }
        };

        let path_for_seek = file.clone();
        let frame = self.videos[id].advance(&path_for_seek, wanted).cloned();
        let revision = self.videos[id].revision;

        let Some(frame) = frame else {
            // Still decoding the first frame — one or two frames of a fresh
            // patch, not an error — unless the worker reported why not.
            let problem = self.videos[id].problem();
            self.nodes.entry(id).unwrap().or_default().status = problem;
            self.ensure_output(id, fallback.0, fallback.1);
            return self.clear_to_black(id, ctx, label);
        };

        // `once` past the end shows black rather than freezing, so a clip
        // that has finished looks finished.
        if ended_message {
            self.ensure_output(id, frame.width, frame.height);
            return self.clear_to_black(id, ctx, label);
        }

        self.upload_frame(id, &frame, revision);
        self.ensure_output(id, frame.width, frame.height);

        let source_view = self.nodes[id].upload.as_ref().unwrap().view.clone();
        let out = self.nodes[id].output.as_ref().unwrap().view.clone();
        let uniforms = self.uniforms(frame.width, frame.height, ctx, [[0.0; 4]; 4]);
        let buf = self.write_uniform(id, 0, uniforms);
        let dummy = self.dummy.clone();
        self.run_pass(Pass {
            label,
            pipeline: PipelineRef::Builtin(ops::NULL),
            target: &out,
            uniform: &buf,
            in0: &source_view,
            in1: &dummy,
            nearest: false,
        })
    }

    /// Copy a decoded frame into this node's upload texture.
    ///
    /// `Rgba8Unorm`, deliberately **not** the sRGB variant. The sRGB format
    /// would have the hardware convert to linear on sample, which sounds
    /// right and is wrong here: nothing else in this pipeline is linear. A
    /// Constant TOP set to 0.5 stores 0.5 and reads back 128, and every
    /// shader does its arithmetic on display-referred values. Converting one
    /// operator and not the others made video — and only video — come out
    /// 2.33× dark: mid-grey 128 went in and 55 came out, which is exactly
    /// the linear value of sRGB 0.5. Consistency with the rest of the engine
    /// beats being right in isolation.
    fn upload_frame(&mut self, id: NodeId, frame: &crate::video::Frame, revision: u64) {
        self.upload_pixels(
            id,
            frame.width,
            frame.height,
            wgpu::TextureFormat::Rgba8Unorm,
            &frame.pixels,
            revision,
        );
    }

    /// Copy CPU pixels into this node's upload texture, reusing it when the
    /// shape has not changed.
    ///
    /// `revision` is what makes a repeated cook cheap: an unchanged decoder
    /// frame is not re-sent. A caller with nothing to compare — the CHOP to
    /// TOP, whose channels are different every frame anyway — passes
    /// `u64::MAX` and always writes.
    fn upload_pixels(
        &mut self,
        id: NodeId,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        bytes: &[u8],
        revision: u64,
    ) {
        let (width, height) = (width.max(1), height.max(1));
        let entry = self.nodes.entry(id).unwrap().or_default();
        let reusable = entry
            .upload
            .as_ref()
            .map(|u| u.width == width && u.height == height && u.format == format)
            .unwrap_or(false);
        if reusable && revision != u64::MAX && entry.upload.as_ref().unwrap().revision == revision {
            return; // Nothing new to send.
        }
        if !reusable {
            let texture = self.ctx.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("otd upload"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.nodes[id].upload = Some(Upload {
                texture,
                view,
                width,
                height,
                format,
                revision: u64::MAX,
            });
        }
        let bytes_per_texel = format.block_copy_size(None).unwrap_or(4);
        let upload = self.nodes[id].upload.as_mut().unwrap();
        upload.revision = revision;
        self.ctx.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &upload.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * bytes_per_texel),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Channels as a texture.
    ///
    /// One row per channel, one column per sample. `Rgba16Float`, not
    /// `Rgba8Unorm`: a channel is not a colour, and clamping it to 0..1 on the
    /// way in would make the operator useless for the thing it is for, which
    /// is handing a shader real numbers. Not `Rgba32Float` either — that
    /// format is not filterable, so it cannot be bound to the one bind group
    /// layout every operator shares, and it would buy nothing anyway when the
    /// texture it lands in is 16-bit float like every other TOP.
    fn cook_chop_to_top(
        &mut self,
        id: NodeId,
        ctx: &CookContext,
        eval: &EvalContext,
        node: &otd_core::Node,
        channels: Option<otd_core::ChannelView<'_>>,
        label: &str,
    ) -> Result<(), CookError> {
        let Some((_, samples, _)) = channels else {
            self.ensure_output(id, 1, 1);
            return self.clear_to_black(id, ctx, label);
        };
        let rgba = node
            .param("layout")
            .map(|p| p.eval(eval).as_str() == "rgba")
            .unwrap_or(false);

        let width = samples.iter().map(|s| s.len()).max().unwrap_or(0).max(1) as u32;
        // In RGBA mode four channels share a row, so the row count is a
        // quarter of the channel count, rounded up.
        let per_row = if rgba { 4 } else { 1 };
        let rows = samples.len().div_ceil(per_row).max(1);

        let mut texels = vec![0.0f32; rows * width as usize * 4];
        for (ci, chan) in samples.iter().enumerate() {
            let (row, comp) = if rgba {
                (ci / 4, ci % 4)
            } else {
                (ci, usize::MAX)
            };
            if row >= rows {
                break;
            }
            for x in 0..width as usize {
                // A channel shorter than the widest is held at its last
                // value rather than dropping to zero, so mixing a
                // single-sample control channel with a waveform broadcasts
                // it the way a CHOP does everywhere else.
                let v = chan
                    .get(x)
                    .copied()
                    .or_else(|| chan.last().copied())
                    .unwrap_or(0.0);
                let base = (row * width as usize + x) * 4;
                if comp == usize::MAX {
                    texels[base] = v;
                    texels[base + 1] = v;
                    texels[base + 2] = v;
                    texels[base + 3] = 1.0;
                } else {
                    texels[base + comp] = v;
                }
            }
        }

        let halves: Vec<u16> = texels.iter().copied().map(crate::f32_to_f16).collect();
        let bytes: &[u8] = bytemuck::cast_slice(&halves);
        self.upload_pixels(id, width, rows as u32, TOP_FORMAT, bytes, u64::MAX);
        self.ensure_output(id, width, rows as u32);

        let source_view = self.nodes[id].upload.as_ref().unwrap().view.clone();
        let out = self.nodes[id].output.as_ref().unwrap().view.clone();
        let uniforms = self.uniforms(width, rows as u32, ctx, [[0.0; 4]; 4]);
        let buf = self.write_uniform(id, 0, uniforms);
        let dummy = self.dummy.clone();
        self.run_pass(Pass {
            label,
            pipeline: PipelineRef::Builtin(ops::NULL),
            target: &out,
            uniform: &buf,
            in0: &source_view,
            in1: &dummy,
            // Nearest: each texel is one sample of one channel, and
            // interpolating between two channels is meaningless.
            nearest: true,
        })
    }

    /// A pass that produces black — what a source with nothing to show
    /// presents, so downstream operators always have a texture.
    fn clear_to_black(
        &mut self,
        id: NodeId,
        ctx: &CookContext,
        label: &str,
    ) -> Result<(), CookError> {
        let (w, h) = {
            let out = self.nodes[id].output.as_ref().unwrap();
            (out.key.width, out.key.height)
        };
        let out = self.nodes[id].output.as_ref().unwrap().view.clone();
        let uniforms = self.uniforms(w, h, ctx, [[0.0; 4]; 4]);
        let buf = self.write_uniform(id, 0, uniforms);
        let dummy = self.dummy.clone();
        self.run_pass(Pass {
            label,
            pipeline: PipelineRef::Builtin(ops::NULL),
            target: &out,
            uniform: &buf,
            in0: &dummy,
            in1: &dummy,
            nearest: false,
        })
    }

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
    graph.find_from(id, path).filter(|t| *t != id)
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
        scene: Option<&dyn Scene>,
        foreign: &Crossings,
    ) -> Result<(), CookError> {
        self.cook_top(graph, id, ctx, eval, scene, foreign)
    }
}

impl Cooker for TopEngine {
    fn extra_inputs(&self, graph: &Graph, id: NodeId) -> Vec<NodeId> {
        let Some(node) = graph.get(id) else {
            return Vec::new();
        };
        // Select reads the current frame, so its target must cook first.
        // Feedback deliberately does not appear here.
        if node.op_type == ops::SELECT {
            return referenced_target(graph, id, "top").into_iter().collect();
        }
        // A Render TOP discovers the Geometry components it draws rather than
        // being wired to them, so it has to declare them. With no Geometry
        // parameter it draws the whole project, and depends on all of it.
        if node.op_type == ops::RENDER {
            let root = node
                .param("geometry")
                .map(|p| p.eval(&EvalContext::default()).as_str())
                .unwrap_or_default();
            let start = if root.trim().is_empty() {
                Some(graph.root())
            } else {
                graph.find(root.trim())
            };
            let Some(start) = start else {
                return Vec::new();
            };
            let mut found = Vec::new();
            let mut stack = vec![start];
            while let Some(cur) = stack.pop() {
                let Some(n) = graph.get(cur) else { continue };
                if n.op_type == crate::scene::GEOMETRY {
                    found.push(cur);
                    continue;
                }
                stack.extend(n.children.iter().copied());
            }
            return found;
        }
        Vec::new()
    }

    fn cook(&mut self, graph: &Graph, id: NodeId, ctx: &CookContext) -> Result<(), CookError> {
        // No CHOPs in sight: parameters resolve against time alone, and
        // there is no geometry to draw.
        let eval = ctx.eval_ctx();
        self.cook_top(graph, id, ctx, &eval, None, &Crossings::new())
    }
}

impl TopEngine {
    fn cook_top(
        &mut self,
        graph: &Graph,
        id: NodeId,
        ctx: &CookContext,
        eval: &EvalContext,
        scene: Option<&dyn Scene>,
        foreign: &Crossings,
    ) -> Result<(), CookError> {
        let node = graph.get(id).ok_or(CookError::NoSuchNode)?;
        let path = graph.path(id);
        // Parameters are evaluated knowing where they live, so `parent.x`
        // resolves against the right component.
        let eval = &EvalContext {
            path: Some(&path),
            ..*eval
        };

        // COMPs hold sub-networks; they have no texture of their own yet.
        if node.family != otd_core::Family::Top {
            return Ok(());
        }

        let spec = ops::spec(&node.op_type)
            .ok_or_else(|| CookError::op(&path, format!("unknown TOP `{}`", node.op_type)))?;
        // ---- An In operator presents whatever is wired to its component from
        // outside. Nothing else in the engine has to know about components.
        if node.connector == otd_core::Connector::In {
            let source = graph.connector_source(id);
            return self.blit_from(graph, id, source, ctx, &path);
        }

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
        // ---- CHOP to TOP: the pixels come from a wire of another family.
        if node.op_type == ops::CHOP_TO_TOP {
            let channels = foreign
                .first()
                .and_then(|c| c.as_ref())
                .and_then(|c| c.as_channels());
            return self.cook_chop_to_top(id, ctx, eval, node, channels, &path);
        }

        // ---- Video: the pixels come from a decoder thread, not a shader.
        if node.op_type == ops::MOVIE_IN || node.op_type == ops::VIDEO_DEVICE_IN {
            return self.cook_video(graph, id, ctx, eval, &path);
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

        // ---- Text is rasterised on the CPU and uploaded, so the shader only
        // has to tint the coverage. That split is what makes Colour a live
        // parameter: changing it is a uniform write, not a re-lay-out.
        if node.op_type == ops::TEXT {
            self.cook_text(id, ctx, eval, node, width, height, &path)?;
            return Ok(());
        }

        // ---- Movie File Out passes its input through like a Null, and the
        // recording is a side effect of having done so. Reading the *output*
        // rather than the input means what lands in the file is exactly what
        // the node's viewer shows.
        if node.op_type == ops::MOVIE_OUT {
            let in0 = self.input_view(graph, id, 0);
            let out = self.nodes[id].output.as_ref().unwrap().view.clone();
            let uniforms = self.uniforms(width, height, ctx, [[0.0; 4]; 4]);
            let buf = self.write_uniform(id, 0, uniforms);
            let dummy = self.dummy.clone();
            self.run_pass(Pass {
                label: &path,
                pipeline: PipelineRef::Builtin(ops::NULL),
                target: &out,
                uniform: &buf,
                in0: &in0,
                in1: &dummy,
                nearest: false,
            })?;
            self.cook_movie_out(id, node, eval);
            return Ok(());
        }

        // ---- The 3D pipeline is a different beast: vertex buffers, a depth
        // attachment, a cull mode. It gets its own module.
        if node.op_type == ops::RENDER {
            return self.cook_render(graph, id, ctx, eval, scene, width, height);
        }

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
