//! The Render TOP's pipeline: depth buffer, instanced draw, one light.
//!
//! Kept apart from the 2D `TopEngine` because it is a genuinely different
//! pipeline — vertex buffers, a depth attachment, a cull mode — and mixing
//! the two would make both harder to read. The engine owns one of these and
//! calls it for `renderTOP` nodes.

use otd_core::{EvalContext, Family, Graph, Node, NodeId};
use wgpu::util::DeviceExt;

use crate::math::{self, Mat4};
use crate::scene::{self, Instance, Scene};
use crate::texture::TOP_FORMAT;

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SceneUniforms {
    view_proj: Mat4,
    model: Mat4,
    camera: [f32; 4],
    light_dir: [f32; 4],
    light_color: [f32; 4],
    base_color: [f32; 4],
    material: [f32; 4],
    render: [f32; 4],
}

/// One thing to draw: geometry, where it is, what it looks like, and how many
/// copies.
pub struct DrawItem {
    pub geometry: otd_sop::Geometry,
    pub model: Mat4,
    pub instances: Vec<Instance>,
    pub base_color: [f32; 4],
    /// metallic, roughness, emit, use_texture — reinterpreted per shading
    /// model; see `material_of`.
    pub material: [f32; 4],
    pub color_map: Option<NodeId>,
    pub shading: scene::Shading,
    /// Draw this item's edges rather than its faces.
    pub wireframe: bool,
}

/// Everything the pass needs, gathered from the graph before any GPU work.
pub struct SceneDescription {
    pub items: Vec<DrawItem>,
    pub view_proj: Mat4,
    pub camera_pos: [f32; 3],
    pub light_dir: [f32; 4],
    pub light_color: [f32; 4],
    pub background: [f32; 4],
    pub ambient: f32,
    pub cull: Option<wgpu::Face>,
    pub wireframe: bool,
}

/// Read the scene out of the graph.
///
/// Deliberately all CPU-side and GPU-free: it is the part worth testing, and
/// it means a mis-wired camera is a wrong matrix rather than a driver error.
pub fn describe(
    graph: &Graph,
    render_node: &Node,
    ctx: &EvalContext,
    scene: &dyn Scene,
    aspect: f32,
) -> SceneDescription {
    let geometry_root = scene::s(render_node, ctx, "geometry");
    let items = collect_items(graph, geometry_root.trim(), ctx, scene);

    let camera_path = scene::s(render_node, ctx, "camera");
    let (view, camera_pos, projection) = camera(graph, camera_path.trim(), ctx, aspect);

    let (light_dir, light_color) = light(graph, scene::s(render_node, ctx, "light").trim(), ctx);

    SceneDescription {
        items,
        view_proj: math::mul(projection, view),
        camera_pos,
        light_dir,
        light_color,
        background: scene::v4(render_node, ctx, "background"),
        ambient: scene::f(render_node, ctx, "ambient"),
        cull: match scene::menu(render_node, ctx, "cull") {
            1 => Some(wgpu::Face::Front),
            2 => None,
            _ => Some(wgpu::Face::Back),
        },
        wireframe: scene::b(render_node, ctx, "wireframe"),
    }
}

/// Every Geometry component under `root` — or the whole project when the
/// Render TOP names nothing, which is the useful default while patching.
fn collect_items(graph: &Graph, root: &str, ctx: &EvalContext, scene: &dyn Scene) -> Vec<DrawItem> {
    let start = if root.is_empty() {
        Some(graph.root())
    } else {
        graph.find(root)
    };
    let Some(start) = start else {
        return Vec::new();
    };

    let mut geometries = Vec::new();
    let mut stack = vec![start];
    while let Some(id) = stack.pop() {
        let Some(node) = graph.get(id) else { continue };
        if node.op_type == scene::GEOMETRY {
            geometries.push(id);
            // A Geometry component's children are its own business.
            continue;
        }
        stack.extend(node.children.iter().copied());
    }
    // Stable order, so a two-object scene does not flicker between frames.
    geometries.sort_by_key(|id| graph.path(*id));

    geometries
        .into_iter()
        .filter_map(|id| {
            let node = graph.get(id)?;
            let sop_path = scene::s(node, ctx, "sop");
            let geometry = scene.geometry(sop_path.trim())?.clone();
            if geometry.is_empty() {
                return None;
            }
            let surface = material_of(graph, &scene::s(node, ctx, "material"), ctx);
            Some(DrawItem {
                geometry,
                model: math::trs(
                    scene::v3(node, ctx, "translate"),
                    scene::v3(node, ctx, "rotate"),
                    scene::v3(node, ctx, "scale"),
                ),
                instances: scene::instances(node, ctx, scene),
                base_color: surface.base_color,
                material: surface.material,
                color_map: surface.color_map,
                shading: surface.shading,
                wireframe: surface.wireframe,
            })
        })
        .collect()
}

/// The three edges of every triangle, as index pairs for a line list.
///
/// Deduplicated, so a closed mesh does not draw each shared edge twice — at a
/// few thousand triangles that is half the work for an identical picture.
fn triangle_edges(geometry: &otd_sop::Geometry) -> Vec<u32> {
    let source: Vec<u32> = if geometry.indices.is_empty() {
        (0..geometry.num_points() as u32).collect()
    } else {
        geometry.indices.clone()
    };
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(source.len() * 2);
    for tri in source.chunks(3) {
        if tri.len() < 3 {
            continue;
        }
        for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
            let key = if a < b { (a, b) } else { (b, a) };
            if seen.insert(key) {
                out.extend([a, b]);
            }
        }
    }
    out
}

/// What a Geometry COMP's Material parameter resolves to.
pub struct Surface {
    pub base_color: [f32; 4],
    /// The four numbers the shader reads; what they mean depends on `shading`.
    pub material: [f32; 4],
    pub color_map: Option<NodeId>,
    pub shading: scene::Shading,
    /// Draw edges rather than faces.
    pub wireframe: bool,
}

impl Default for Surface {
    fn default() -> Self {
        Surface {
            base_color: [0.8, 0.8, 0.85, 1.0],
            material: [0.0, 0.4, 0.0, 0.0],
            color_map: None,
            shading: scene::Shading::Pbr,
            wireframe: false,
        }
    }
}

fn material_of(graph: &Graph, path: &str, ctx: &EvalContext) -> Surface {
    let path = path.trim();
    if path.is_empty() {
        return Surface::default();
    }
    let Some(node) = graph.find(path).and_then(|id| graph.get(id)) else {
        return Surface::default();
    };
    if node.family != Family::Mat {
        return Surface::default();
    }
    // A TOP wired into the material's input is its colour map.
    let map = node.inputs.first().copied().flatten();
    let has_map = if map.is_some() { 1.0 } else { 0.0 };
    let base_color = scene::v4(node, ctx, "basecolor");
    let emit = scene::f(node, ctx, "emit");

    match node.op_type.as_str() {
        scene::CONSTANT_MAT => Surface {
            base_color,
            // A Constant material's Brightness is the emit term, since emit is
            // the only one of the four that survives with the lighting off.
            material: [0.0, 0.0, emit, has_map],
            color_map: map,
            shading: scene::Shading::Constant,
            wireframe: false,
        },
        scene::WIREFRAME => Surface {
            base_color,
            material: [0.0, 0.0, emit, 0.0],
            color_map: None,
            shading: scene::Shading::Constant,
            wireframe: true,
        },
        scene::PHONG => Surface {
            base_color,
            material: [
                scene::f(node, ctx, "specular"),
                scene::f(node, ctx, "shininess"),
                emit,
                has_map,
            ],
            color_map: map,
            shading: scene::Shading::Phong,
            wireframe: false,
        },
        _ => Surface {
            base_color,
            material: [
                scene::f(node, ctx, "metallic"),
                scene::f(node, ctx, "roughness"),
                emit,
                has_map,
            ],
            color_map: map,
            shading: scene::Shading::Pbr,
            wireframe: false,
        },
    }
}

/// The view and projection matrices for a camera, with a sensible default so
/// a Render TOP with no camera still shows something.
fn camera(graph: &Graph, path: &str, ctx: &EvalContext, aspect: f32) -> (Mat4, [f32; 3], Mat4) {
    let node = if path.is_empty() {
        None
    } else {
        graph.find(path).and_then(|id| graph.get(id))
    };
    let Some(node) = node.filter(|n| n.op_type == scene::CAMERA) else {
        let eye = [0.0, 0.0, 5.0];
        return (
            math::look_at(eye, [0.0; 3], [0.0, 1.0, 0.0]),
            eye,
            math::perspective(45.0, aspect, 0.1, 200.0),
        );
    };

    let translate = scene::v3(node, ctx, "translate");
    let rotate = scene::v3(node, ctx, "rotate");
    let look_at_path = scene::s(node, ctx, "lookat");

    // Aiming at another component beats dialling in Euler angles, and it is
    // what makes an orbiting camera one expression rather than three.
    let view = match graph
        .find(look_at_path.trim())
        .and_then(|id| graph.get(id))
        .filter(|_| !look_at_path.trim().is_empty())
    {
        Some(target) => math::look_at(
            translate,
            scene::v3(target, ctx, "translate"),
            [0.0, 1.0, 0.0],
        ),
        None => math::inverse_trs(translate, rotate, [1.0; 3]),
    };

    let near = scene::f(node, ctx, "near").max(1e-3);
    let far = scene::f(node, ctx, "far").max(near + 1e-3);
    let projection = if scene::menu(node, ctx, "projection") == 1 {
        math::orthographic(scene::f(node, ctx, "orthosize"), aspect, near, far)
    } else {
        math::perspective(scene::f(node, ctx, "fov"), aspect, near, far)
    };
    (view, translate, projection)
}

fn light(graph: &Graph, path: &str, ctx: &EvalContext) -> ([f32; 4], [f32; 4]) {
    let node = if path.is_empty() {
        None
    } else {
        graph.find(path).and_then(|id| graph.get(id))
    };
    let Some(node) = node.filter(|n| n.op_type == scene::LIGHT) else {
        // A key light over the viewer's shoulder, so an unlit scene is still
        // readable while patching.
        let d = normalize([0.4, 0.7, 0.6]);
        return ([d[0], d[1], d[2], 1.0], [1.0; 4]);
    };
    // The light points from its position at the origin, so moving it is the
    // whole interaction.
    let d = normalize(scene::v3(node, ctx, "translate"));
    (
        [d[0], d[1], d[2], scene::f(node, ctx, "intensity")],
        scene::v4(node, ctx, "color"),
    )
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len < 1e-9 {
        [0.0, 1.0, 0.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

// ------------------------------------------------------------- the pass

pub struct Renderer {
    pipelines: std::collections::HashMap<(bool, u8), wgpu::RenderPipeline>,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    module: wgpu::ShaderModule,
    sampler: wgpu::Sampler,
    pub draws_this_frame: u32,
    pub instances_this_frame: u32,
}

const VERTEX_STRIDE: u64 = 12 * 4;
const INSTANCE_STRIDE: u64 = 13 * 4;

impl Renderer {
    pub fn new(device: &wgpu::Device) -> Renderer {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("otd render3d bind layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
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
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("otd render3d pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("otd render3d"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/render3d.wgsl").into()),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("otd render3d sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Renderer {
            pipelines: std::collections::HashMap::new(),
            layout,
            pipeline_layout,
            module,
            sampler,
            draws_this_frame: 0,
            instances_this_frame: 0,
        }
    }

    fn pipeline(
        &mut self,
        device: &wgpu::Device,
        lines: bool,
        cull: Option<wgpu::Face>,
    ) -> &wgpu::RenderPipeline {
        let key = (
            lines,
            match cull {
                Some(wgpu::Face::Back) => 0u8,
                Some(wgpu::Face::Front) => 1,
                None => 2,
            },
        );
        self.pipelines.entry(key).or_insert_with(|| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("otd render3d"),
                layout: Some(&self.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &self.module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[
                        wgpu::VertexBufferLayout {
                            array_stride: VERTEX_STRIDE,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &wgpu::vertex_attr_array![
                                0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x4
                            ],
                        },
                        wgpu::VertexBufferLayout {
                            array_stride: INSTANCE_STRIDE,
                            step_mode: wgpu::VertexStepMode::Instance,
                            attributes: &wgpu::vertex_attr_array![
                                4 => Float32x3, 5 => Float32x3, 6 => Float32x3, 7 => Float32x4
                            ],
                        },
                    ],
                },
                primitive: wgpu::PrimitiveState {
                    topology: if lines {
                        wgpu::PrimitiveTopology::LineList
                    } else {
                        wgpu::PrimitiveTopology::TriangleList
                    },
                    cull_mode: if lines { None } else { cull },
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &self.module,
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
            })
        })
    }

    /// Record the scene into an encoder.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        description: &SceneDescription,
        dummy_texture: &wgpu::TextureView,
        map_of: &dyn Fn(NodeId) -> Option<wgpu::TextureView>,
    ) {
        // Buffers and bind groups are built before the pass, because a render
        // pass borrows the encoder for its whole life.
        struct Prepared {
            vertices: wgpu::Buffer,
            indices: Option<(wgpu::Buffer, u32)>,
            instances: wgpu::Buffer,
            vertex_count: u32,
            instance_count: u32,
            bind: wgpu::BindGroup,
            lines: bool,
        }

        let mut prepared = Vec::with_capacity(description.items.len());
        for item in &description.items {
            if item.instances.is_empty() {
                continue;
            }
            let vertex_data = item.geometry.vertex_bytes();
            if vertex_data.is_empty() {
                continue;
            }
            let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("otd geometry"),
                contents: bytemuck::cast_slice(&vertex_data),
                usage: wgpu::BufferUsages::VERTEX,
            });
            // Drawing a triangle index list as a line list would pair the
            // indices up as they come — a-b, c-a, c-d — which silently loses
            // one edge of every triangle. Expanding to real edge pairs is the
            // difference between a wireframe and a mesh with holes in it.
            // Per item as well as per render: a Wireframe MAT on one object
            // in a lit scene is the useful case, and the Render TOP's own
            // flag is the "show me everything as edges" override.
            let lines = description.wireframe
                || item.wireframe
                || item.geometry.topology != otd_sop::Topology::Triangles;
            let edges = (lines && item.geometry.topology == otd_sop::Topology::Triangles)
                .then(|| triangle_edges(&item.geometry));

            let index_data: Option<&[u32]> = match (&edges, item.geometry.indices.is_empty()) {
                (Some(e), _) => Some(e),
                (None, false) => Some(&item.geometry.indices),
                (None, true) => None,
            };
            let indices = index_data.map(|data| {
                (
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("otd geometry indices"),
                        contents: bytemuck::cast_slice(data),
                        usage: wgpu::BufferUsages::INDEX,
                    }),
                    data.len() as u32,
                )
            });
            let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("otd instances"),
                contents: bytemuck::cast_slice(&item.instances),
                usage: wgpu::BufferUsages::VERTEX,
            });

            let uniforms = SceneUniforms {
                view_proj: description.view_proj,
                model: item.model,
                camera: [
                    description.camera_pos[0],
                    description.camera_pos[1],
                    description.camera_pos[2],
                    0.0,
                ],
                light_dir: description.light_dir,
                light_color: description.light_color,
                base_color: item.base_color,
                material: item.material,
                render: [description.ambient, 1.0, item.shading as u8 as f32, 0.0],
            };
            let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("otd render3d uniforms"),
                contents: bytemuck::bytes_of(&uniforms),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let map = item
                .color_map
                .and_then(map_of)
                .unwrap_or_else(|| dummy_texture.clone());
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("otd render3d"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&map),
                    },
                ],
            });

            prepared.push(Prepared {
                vertices,
                indices,
                instances,
                vertex_count: item.geometry.num_vertices() as u32,
                instance_count: item.instances.len() as u32,
                bind,
                lines,
            });
        }

        // Pipelines are created up front for the same borrow reason.
        let pipelines: Vec<wgpu::RenderPipeline> = prepared
            .iter()
            .map(|p| self.pipeline(device, p.lines, description.cull).clone())
            .collect();

        let bg = description.background;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("otd render3d"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: bg[0] as f64,
                        g: bg[1] as f64,
                        b: bg[2] as f64,
                        a: bg[3] as f64,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        for (item, pipeline) in prepared.iter().zip(&pipelines) {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &item.bind, &[]);
            pass.set_vertex_buffer(0, item.vertices.slice(..));
            pass.set_vertex_buffer(1, item.instances.slice(..));
            match &item.indices {
                Some((buffer, count)) => {
                    pass.set_index_buffer(buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..*count, 0, 0..item.instance_count);
                }
                None => pass.draw(0..item.vertex_count, 0..item.instance_count),
            }
            self.draws_this_frame += 1;
            self.instances_this_frame += item.instance_count;
        }
    }

    pub fn begin_frame(&mut self) {
        self.draws_this_frame = 0;
        self.instances_this_frame = 0;
    }
}

/// A depth texture matched to a colour target.
pub fn depth_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("otd depth"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}
