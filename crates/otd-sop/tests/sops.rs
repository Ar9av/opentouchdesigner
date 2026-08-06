//! Behaviour tests for the SOP family.

use otd_core::{CookContext, CookEngine, Graph, NodeId, OpRegistry, Value};
use otd_sop::{Geometry, SopHost, Topology, ops};

struct Patch {
    graph: Graph,
    reg: OpRegistry,
    host: SopHost,
    cook: CookEngine,
    time: CookContext,
}

impl Patch {
    fn new() -> Self {
        Patch {
            graph: Graph::new(),
            reg: ops::registry(),
            host: SopHost::new(),
            cook: CookEngine::new(),
            time: CookContext::default(),
        }
    }
    fn add(&mut self, op: &str, name: &str) -> NodeId {
        let root = self.graph.root();
        let def = self.reg.get(op).unwrap_or_else(|| panic!("{op}")).clone();
        self.graph.create(root, &def, Some(name)).unwrap()
    }
    fn set(&mut self, id: NodeId, key: &str, v: Value) {
        self.graph.set_param(id, key, v).unwrap();
    }
    /// One frame. The cook engine resolves each node at most once per frame,
    /// so a test that wants to see a re-cook has to move to the next one.
    fn run(&mut self, root: NodeId) {
        self.cook
            .cook_frame(&self.graph, &[root], &self.time, &mut self.host)
            .unwrap();
        self.time.advance(1.0 / 60.0);
    }
    fn geo(&self, id: NodeId) -> &Geometry {
        self.host.geometry(id).expect("cooked")
    }
}

#[test]
fn a_box_has_six_flat_faces() {
    let mut p = Patch::new();
    let b = p.add("boxSOP", "box1");
    p.run(b);
    let g = p.geo(b);

    // Four corners per face so each face can carry its own normal — a shared
    // eight-point cube cannot be flat shaded.
    assert_eq!(g.num_points(), 24);
    assert_eq!(g.num_primitives(), 12);

    let (lo, hi) = g.bounds();
    assert_eq!(lo, [-0.5, -0.5, -0.5]);
    assert_eq!(hi, [0.5, 0.5, 0.5]);

    // Every normal is a unit axis vector.
    for point in &g.points {
        let len: f32 = point.normal.iter().map(|c| c * c).sum::<f32>().sqrt();
        assert!((len - 1.0).abs() < 1e-5, "{:?}", point.normal);
    }
}

#[test]
fn a_sphere_puts_every_point_on_its_radius() {
    let mut p = Patch::new();
    let s = p.add("sphereSOP", "sphere1");
    p.set(s, "radius", Value::Float(2.0));
    p.set(s, "rows", Value::Int(8));
    p.set(s, "columns", Value::Int(12));
    p.run(s);
    let g = p.geo(s);

    assert_eq!(g.num_points(), 9 * 13);
    for point in &g.points {
        let r: f32 = point.position.iter().map(|c| c * c).sum::<f32>().sqrt();
        assert!((r - 2.0).abs() < 1e-4, "point at radius {r}");
    }
    assert_eq!(g.num_primitives(), 8 * 12 * 2);
}

#[test]
fn a_grid_faces_the_way_it_is_told_to() {
    let mut p = Patch::new();
    let g = p.add("gridSOP", "grid1");
    p.set(g, "rows", Value::Int(3));
    p.set(g, "columns", Value::Int(3));
    p.run(g);
    assert_eq!(p.geo(g).num_points(), 9);
    assert_eq!(p.geo(g).points[0].normal, [0.0, 0.0, 1.0]);

    p.set(g, "orientation", Value::Str("xz".into()));
    p.run(g);
    assert_eq!(p.geo(g).points[0].normal, [0.0, 1.0, 0.0]);
    // Laid flat, every point shares a height.
    assert!(p.geo(g).points.iter().all(|pt| pt.position[1] == 0.0));
}

#[test]
fn transform_moves_points_and_rotates_normals_without_translating_them() {
    let mut p = Patch::new();
    let g = p.add("gridSOP", "grid1");
    let t = p.add("transformSOP", "xform1");
    p.graph.connect(g, t, 0).unwrap();
    p.set(t, "translate", Value::Vec3([10.0, 0.0, 0.0]));
    p.set(t, "rotate", Value::Vec3([0.0, 90.0, 0.0]));
    p.run(t);

    let g = p.geo(t);
    let (lo, hi) = g.bounds();
    assert!((lo[0] - 10.0).abs() < 0.6, "moved to x=10: {lo:?}");
    assert!((hi[0] - 10.0).abs() < 0.6);

    // A +Z normal rotated 90° about Y points along +X.
    let n = g.points[0].normal;
    assert!((n[0] - 1.0).abs() < 1e-5, "normal became {n:?}");
    assert!(n[1].abs() < 1e-5 && n[2].abs() < 1e-5);
}

#[test]
fn noise_displaces_points_but_keeps_the_topology() {
    let mut p = Patch::new();
    let g = p.add("gridSOP", "grid1");
    let n = p.add("noiseSOP", "noise1");
    p.graph.connect(g, n, 0).unwrap();
    p.set(n, "amplitude", Value::Float(1.0));
    p.run(n);

    let flat = p.geo(g).clone();
    let bumpy = p.geo(n);
    assert_eq!(flat.num_points(), bumpy.num_points());
    assert_eq!(flat.indices, bumpy.indices);
    assert_ne!(flat.points, bumpy.points, "nothing moved");

    // Displacing along +Z normals only moves Z.
    for (a, b) in flat.points.iter().zip(&bumpy.points) {
        assert_eq!(a.position[0], b.position[0]);
        assert_eq!(a.position[1], b.position[1]);
    }
}

#[test]
fn noise_is_deterministic() {
    let mut p = Patch::new();
    let g = p.add("gridSOP", "grid1");
    let n = p.add("noiseSOP", "noise1");
    p.graph.connect(g, n, 0).unwrap();
    p.run(n);
    let first = p.geo(n).clone();

    let mut q = Patch::new();
    let g2 = q.add("gridSOP", "grid1");
    let n2 = q.add("noiseSOP", "noise1");
    q.graph.connect(g2, n2, 0).unwrap();
    q.run(n2);
    assert_eq!(
        &first,
        q.geo(n2),
        "the same patch must give the same points"
    );
}

#[test]
fn copy_stamps_a_compounding_transform() {
    let mut p = Patch::new();
    let b = p.add("boxSOP", "box1");
    let c = p.add("copySOP", "copy1");
    p.graph.connect(b, c, 0).unwrap();
    p.set(c, "count", Value::Int(4));
    p.set(c, "translate", Value::Vec3([2.0, 0.0, 0.0]));
    p.run(c);

    let g = p.geo(c);
    assert_eq!(g.num_points(), 24 * 4);
    let (lo, hi) = g.bounds();
    assert!((lo[0] + 0.5).abs() < 1e-4, "first copy at the origin");
    assert!((hi[0] - 6.5).abs() < 1e-4, "fourth copy at x=6: {hi:?}");
}

#[test]
fn merge_keeps_both_halves_separate() {
    let mut p = Patch::new();
    let a = p.add("boxSOP", "box1");
    let b = p.add("sphereSOP", "sphere1");
    let m = p.add("mergeSOP", "merge1");
    p.graph.connect(a, m, 0).unwrap();
    p.graph.connect(b, m, 1).unwrap();
    p.run(m);

    let g = p.geo(m);
    assert_eq!(
        g.num_points(),
        p.geo(a).num_points() + p.geo(b).num_points()
    );
    assert_eq!(
        g.num_primitives(),
        p.geo(a).num_primitives() + p.geo(b).num_primitives()
    );
    // Every index is in range — an off-by-one here would render garbage.
    assert!(g.indices.iter().all(|i| (*i as usize) < g.num_points()));
}

#[test]
fn a_line_is_points_not_triangles() {
    let mut p = Patch::new();
    let l = p.add("lineSOP", "line1");
    p.set(l, "points", Value::Int(5));
    p.run(l);
    let g = p.geo(l);
    assert_eq!(g.topology, Topology::Lines);
    assert_eq!(g.num_points(), 5);
    assert_eq!(g.points[0].position, [-1.0, 0.0, 0.0]);
    assert_eq!(g.points[4].position, [1.0, 0.0, 0.0]);
}

#[test]
fn a_static_geometry_chain_cooks_once() {
    let mut p = Patch::new();
    let g = p.add("gridSOP", "grid1");
    let n = p.add("noiseSOP", "noise1");
    p.graph.connect(g, n, 0).unwrap();
    for _ in 0..10 {
        p.run(n);
    }
    assert_eq!(p.cook.cook_count(g), 1);
    assert_eq!(p.cook.cook_count(n), 1);
}

#[test]
fn every_sop_cooks_without_panicking() {
    let mut p = Patch::new();
    for spec in ops::all() {
        let id = p.add(spec.def.type_name, spec.def.type_name);
        p.run(id);
        assert!(
            p.host.geometry(id).is_some(),
            "{} produced nothing",
            spec.def.type_name
        );
    }
}
