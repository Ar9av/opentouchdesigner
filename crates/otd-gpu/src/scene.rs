//! The 3D scene: Geometry, Camera and Light components, materials, and the
//! instance data that feeds the renderer.
//!
//! PLAN.md Phase 4 names "instancing from CHOP/DAT/TOP sources (texture-based
//! instancing = the signature TD trick)" as the thing to get right. Both
//! paths are here and both end in the same place — a per-instance transform
//! the vertex shader applies:
//!
//!  * **From a CHOP**: each *sample* is an instance. A Pattern CHOP of 2000
//!    samples is 2000 instances, and the channels are named by parameter.
//!  * **From a TOP**: the texture is read in the vertex shader by instance
//!    index. Nothing comes back to the CPU, which is the whole point — a
//!    feedback loop can compute a million positions and none of them cross
//!    the bus.

use otd_core::indexmap::IndexMap;
use otd_core::{Connector, EvalContext, Family, Node, OpDef, OpRegistry, Param, Value};
use otd_sop::Geometry;

/// What the renderer needs from the rest of the network.
///
/// Implemented by the cross-family engine; `otd-gpu` never looks up a path
/// itself.
pub trait Scene {
    /// The geometry a SOP (or a component wrapping one) is presenting.
    fn geometry(&self, path: &str) -> Option<&Geometry>;
    /// A CHOP's channels, by name, with all their samples.
    fn channels(&self, path: &str) -> Option<Vec<(String, Vec<f32>)>>;
}

/// One instance, in the layout the instance vertex buffer wants.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    pub translate: [f32; 3],
    pub scale: [f32; 3],
    pub rotate: [f32; 3],
    pub color: [f32; 4],
}

impl Default for Instance {
    fn default() -> Self {
        Instance {
            translate: [0.0; 3],
            scale: [1.0; 3],
            rotate: [0.0; 3],
            color: [1.0; 4],
        }
    }
}

fn val(node: &Node, ctx: &EvalContext, key: &str) -> Value {
    node.param(key)
        .map(|p| p.eval(ctx))
        .unwrap_or(Value::Float(0.0))
}

pub fn f(node: &Node, ctx: &EvalContext, key: &str) -> f32 {
    val(node, ctx, key).as_f32()
}

pub fn b(node: &Node, ctx: &EvalContext, key: &str) -> bool {
    val(node, ctx, key).as_bool()
}

pub fn s(node: &Node, ctx: &EvalContext, key: &str) -> String {
    val(node, ctx, key).as_str()
}

pub fn v3(node: &Node, ctx: &EvalContext, key: &str) -> [f32; 3] {
    let v = val(node, ctx, key).as_vec4_f32();
    [v[0], v[1], v[2]]
}

pub fn v4(node: &Node, ctx: &EvalContext, key: &str) -> [f32; 4] {
    val(node, ctx, key).as_vec4_f32()
}

pub fn menu(node: &Node, ctx: &EvalContext, key: &str) -> usize {
    let Some(p) = node.param(key) else { return 0 };
    let chosen = p.eval(ctx).as_str();
    p.menu
        .as_ref()
        .and_then(|m| m.iter().position(|i| *i == chosen))
        .unwrap_or(0)
}

/// Build the instance list for a Geometry COMP.
///
/// Returns a single identity instance when instancing is off, so the render
/// path has one shape rather than two.
pub fn instances(node: &Node, ctx: &EvalContext, scene: &dyn Scene) -> Vec<Instance> {
    if !b(node, ctx, "instancing") {
        return vec![Instance::default()];
    }
    let path = s(node, ctx, "instancechop");
    let Some(channels) = scene.channels(path.trim()) else {
        return vec![Instance::default()];
    };

    let pick = |name: &str| -> Option<&Vec<f32>> {
        if name.trim().is_empty() {
            return None;
        }
        channels
            .iter()
            .find(|(n, _)| n == name.trim())
            .map(|(_, v)| v)
    };

    // Each sample is an instance, so the count is the longest channel that is
    // actually referenced.
    let names = [
        s(node, ctx, "tx"),
        s(node, ctx, "ty"),
        s(node, ctx, "tz"),
        s(node, ctx, "sx"),
        s(node, ctx, "sy"),
        s(node, ctx, "sz"),
        s(node, ctx, "rx"),
        s(node, ctx, "ry"),
        s(node, ctx, "rz"),
        s(node, ctx, "cr"),
        s(node, ctx, "cg"),
        s(node, ctx, "cb"),
        s(node, ctx, "ca"),
    ];
    let count = names
        .iter()
        .filter_map(|n| pick(n))
        .map(|c| c.len())
        .max()
        .unwrap_or(0);
    if count == 0 {
        return vec![Instance::default()];
    }

    let sample = |channel: Option<&Vec<f32>>, i: usize, default: f32| -> f32 {
        match channel {
            // A single-sample channel broadcasts to every instance, which is
            // what makes "one LFO scales all of them" work.
            Some(c) if !c.is_empty() => c[i.min(c.len() - 1)],
            _ => default,
        }
    };
    let chans: Vec<Option<&Vec<f32>>> = names.iter().map(|n| pick(n)).collect();
    let uniform_scale = f(node, ctx, "instancescale");

    (0..count.min(1 << 20))
        .map(|i| Instance {
            translate: [
                sample(chans[0], i, 0.0),
                sample(chans[1], i, 0.0),
                sample(chans[2], i, 0.0),
            ],
            scale: [
                sample(chans[3], i, 1.0) * uniform_scale,
                sample(chans[4], i, 1.0) * uniform_scale,
                sample(chans[5], i, 1.0) * uniform_scale,
            ],
            rotate: [
                sample(chans[6], i, 0.0),
                sample(chans[7], i, 0.0),
                sample(chans[8], i, 0.0),
            ],
            color: [
                sample(chans[9], i, 1.0),
                sample(chans[10], i, 1.0),
                sample(chans[11], i, 1.0),
                sample(chans[12], i, 1.0),
            ],
        })
        .collect()
}

// -------------------------------------------------------- operator table

macro_rules! params {
    ($($key:expr => $param:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut m: IndexMap<String, Param> = IndexMap::new();
        $( m.insert($key.into(), $param); )*
        m
    }};
}

fn params_geometry() -> IndexMap<String, Param> {
    params! {
        "sop" => Param::str("").with_label("SOP").as_path_ref(),
        "material" => Param::str("").with_label("Material").as_path_ref(),
        "translate" => Param::xyz([0.0, 0.0, 0.0]).with_label("Translate"),
        "rotate" => Param::xyz([0.0, 0.0, 0.0]).with_label("Rotate"),
        "scale" => Param::xyz([1.0, 1.0, 1.0]).with_label("Scale"),
        "instancing" => Param::bool(false).with_label("Instancing"),
        "instancechop" => Param::str("").with_label("Instance CHOP").as_path_ref(),
        "instancetop" => Param::str("").with_label("Instance TOP").as_path_ref(),
        "instancescale" => Param::float(1.0).with_label("Instance Scale").with_range(0.0, 10.0),
        "instancecount" => Param::int(1024).with_label("Instance Count (TOP)").with_range(1.0, 1000000.0),
        "tx" => Param::str("tx").with_label("Translate X Channel"),
        "ty" => Param::str("ty").with_label("Translate Y Channel"),
        "tz" => Param::str("tz").with_label("Translate Z Channel"),
        "sx" => Param::str("").with_label("Scale X Channel"),
        "sy" => Param::str("").with_label("Scale Y Channel"),
        "sz" => Param::str("").with_label("Scale Z Channel"),
        "rx" => Param::str("").with_label("Rotate X Channel"),
        "ry" => Param::str("").with_label("Rotate Y Channel"),
        "rz" => Param::str("").with_label("Rotate Z Channel"),
        "cr" => Param::str("").with_label("Red Channel"),
        "cg" => Param::str("").with_label("Green Channel"),
        "cb" => Param::str("").with_label("Blue Channel"),
        "ca" => Param::str("").with_label("Alpha Channel"),
    }
}

fn params_camera() -> IndexMap<String, Param> {
    params! {
        "translate" => Param::xyz([0.0, 0.0, 5.0]).with_label("Translate"),
        "rotate" => Param::xyz([0.0, 0.0, 0.0]).with_label("Rotate"),
        "lookat" => Param::str("").with_label("Look At").as_path_ref(),
        "projection" => Param::menu("perspective", &["perspective", "orthographic"])
            .with_label("Projection"),
        "fov" => Param::float(45.0).with_label("Field of View").with_range(5.0, 170.0),
        "orthosize" => Param::float(4.0).with_label("Ortho Height").with_range(0.1, 100.0),
        "near" => Param::float(0.1).with_label("Near").with_range(0.001, 10.0),
        "far" => Param::float(200.0).with_label("Far").with_range(1.0, 10000.0),
    }
}

fn params_light() -> IndexMap<String, Param> {
    params! {
        "translate" => Param::xyz([3.0, 5.0, 4.0]).with_label("Translate"),
        "color" => Param::rgba([1.0, 1.0, 1.0, 1.0]).with_label("Color"),
        "intensity" => Param::float(1.0).with_label("Intensity").with_range(0.0, 8.0),
    }
}

fn params_pbr() -> IndexMap<String, Param> {
    params! {
        "basecolor" => Param::rgba([0.8, 0.8, 0.85, 1.0]).with_label("Base Color"),
        "metallic" => Param::float(0.0).with_label("Metallic").with_range(0.0, 1.0),
        "roughness" => Param::float(0.4).with_label("Roughness").with_range(0.02, 1.0),
        "emit" => Param::float(0.0).with_label("Emit").with_range(0.0, 4.0),
    }
}

pub const GEOMETRY: &str = "geometryCOMP";
pub const CAMERA: &str = "cameraCOMP";
pub const LIGHT: &str = "lightCOMP";
pub const PBR: &str = "pbrMAT";

/// The scene components and materials.
///
/// Geometry, Camera and Light are components: they hold settings and are
/// referenced by a Render TOP, rather than producing anything themselves.
pub fn defs() -> Vec<OpDef> {
    vec![
        OpDef {
            type_name: GEOMETRY,
            label: "Geometry",
            family: Family::Comp,
            inputs: &[],
            summary: "Places a SOP in the scene, optionally instanced.",
            time_dependent: false,
            params: params_geometry,
            connector: Connector::None,
        },
        OpDef {
            type_name: CAMERA,
            label: "Camera",
            family: Family::Comp,
            inputs: &[],
            summary: "A viewpoint for a Render TOP.",
            time_dependent: false,
            params: params_camera,
            connector: Connector::None,
        },
        OpDef {
            type_name: LIGHT,
            label: "Light",
            family: Family::Comp,
            inputs: &[],
            summary: "A directional light, aimed from its position at the origin.",
            time_dependent: false,
            params: params_light,
            connector: Connector::None,
        },
        OpDef {
            type_name: PBR,
            label: "PBR",
            family: Family::Mat,
            inputs: &["color"],
            summary: "Base colour, metallic, roughness and emission, with an optional map.",
            time_dependent: false,
            params: params_pbr,
            connector: Connector::None,
        },
    ]
}

pub fn register(registry: &mut OpRegistry) {
    for def in defs() {
        registry.register(def);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use otd_core::Graph;

    struct FakeScene(Vec<(String, Vec<f32>)>);
    impl Scene for FakeScene {
        fn geometry(&self, _path: &str) -> Option<&Geometry> {
            None
        }
        fn channels(&self, path: &str) -> Option<Vec<(String, Vec<f32>)>> {
            (path == "/pos").then(|| self.0.clone())
        }
    }

    fn geometry_node(graph: &mut Graph) -> otd_core::NodeId {
        let mut reg = OpRegistry::new();
        register(&mut reg);
        let root = graph.root();
        graph
            .create(root, reg.get(GEOMETRY).unwrap(), Some("geo1"))
            .unwrap()
    }

    #[test]
    fn instancing_off_gives_exactly_one_instance() {
        let mut graph = Graph::new();
        let id = geometry_node(&mut graph);
        let scene = FakeScene(vec![]);
        let list = instances(graph.node(id), &EvalContext::default(), &scene);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].scale, [1.0; 3]);
    }

    #[test]
    fn each_sample_of_a_chop_is_an_instance() {
        let mut graph = Graph::new();
        let id = geometry_node(&mut graph);
        graph
            .set_param(id, "instancing", Value::Bool(true))
            .unwrap();
        graph
            .set_param(id, "instancechop", Value::Str("/pos".into()))
            .unwrap();

        let scene = FakeScene(vec![
            ("tx".into(), vec![0.0, 1.0, 2.0, 3.0]),
            ("ty".into(), vec![10.0, 11.0, 12.0, 13.0]),
        ]);
        let list = instances(graph.node(id), &EvalContext::default(), &scene);
        assert_eq!(list.len(), 4);
        assert_eq!(list[2].translate, [2.0, 12.0, 0.0]);
        // Unreferenced channels keep their defaults.
        assert_eq!(list[2].scale, [1.0; 3]);
    }

    #[test]
    fn a_single_sample_channel_broadcasts_across_instances() {
        let mut graph = Graph::new();
        let id = geometry_node(&mut graph);
        graph
            .set_param(id, "instancing", Value::Bool(true))
            .unwrap();
        graph
            .set_param(id, "instancechop", Value::Str("/pos".into()))
            .unwrap();
        graph
            .set_param(id, "sx", Value::Str("size".into()))
            .unwrap();

        let scene = FakeScene(vec![
            ("tx".into(), vec![0.0, 1.0, 2.0]),
            // One sample: the "all of them scale together" case.
            ("size".into(), vec![0.25]),
        ]);
        let list = instances(graph.node(id), &EvalContext::default(), &scene);
        assert_eq!(list.len(), 3);
        assert!(list.iter().all(|i| i.scale[0] == 0.25));
    }

    #[test]
    fn a_missing_instance_chop_falls_back_to_one_instance() {
        let mut graph = Graph::new();
        let id = geometry_node(&mut graph);
        graph
            .set_param(id, "instancing", Value::Bool(true))
            .unwrap();
        graph
            .set_param(id, "instancechop", Value::Str("/nowhere".into()))
            .unwrap();
        let scene = FakeScene(vec![]);
        assert_eq!(
            instances(graph.node(id), &EvalContext::default(), &scene).len(),
            1
        );
    }

    #[test]
    fn a_geometry_comps_references_are_cook_dependencies() {
        let mut graph = Graph::new();
        let id = geometry_node(&mut graph);
        graph
            .set_param(id, "sop", Value::Str("/sphere1".into()))
            .unwrap();
        graph
            .set_param(id, "instancechop", Value::Str("/pos".into()))
            .unwrap();
        let sources: Vec<&str> = graph.node(id).param_sources().collect();
        assert!(sources.contains(&"/sphere1"), "{sources:?}");
        assert!(sources.contains(&"/pos"), "{sources:?}");
    }
}
