//! The 3D pipeline: a Render TOP drawing Geometry components through a
//! Camera, with instancing driven by channels.

use otd_core::{CookContext, CookEngine, Graph, NodeId, OpRegistry, Value};
use otd_engine::{Engines, registry};
use otd_gpu::{GpuContext, read_pixels_rgba8};

macro_rules! gpu_or_skip {
    () => {
        match GpuContext::headless() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("skipping: no GPU available ({e})");
                return;
            }
        }
    };
}

struct Rig {
    graph: Graph,
    reg: OpRegistry,
    engines: Engines,
    cook: CookEngine,
    time: CookContext,
    gpu: GpuContext,
}

impl Rig {
    fn new(gpu: GpuContext) -> Self {
        Rig {
            graph: Graph::new(),
            reg: registry(),
            engines: Engines::new(gpu.clone()),
            cook: CookEngine::new(),
            time: CookContext::default(),
            gpu,
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
    fn run(&mut self, root: NodeId) {
        self.engines.begin_frame();
        self.cook
            .cook_frame(&self.graph, &[root], &self.time, &mut self.engines)
            .unwrap();
        self.engines.end_frame();
        self.time.advance(1.0 / 60.0);
    }
    fn pixels(&self, id: NodeId) -> (u32, u32, Vec<u8>) {
        let tex = self
            .engines
            .top
            .output(&self.graph, id)
            .expect("the Render TOP produced a texture")
            .clone();
        read_pixels_rgba8(&self.gpu, &tex).unwrap()
    }
    /// How many pixels are brighter than the background.
    fn lit(&self, id: NodeId) -> usize {
        let (_, _, px) = self.pixels(id);
        px.chunks(4)
            .filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 30)
            .count()
    }
}

/// sphere -> geometry component -> render, with a camera and a light.
fn basic_scene(rig: &mut Rig) -> NodeId {
    rig.add("sphereSOP", "sphere1");
    let geo = rig.add("geometryCOMP", "geo1");
    let cam = rig.add("cameraCOMP", "cam1");
    rig.add("lightCOMP", "light1");
    let render = rig.add("renderTOP", "render1");

    rig.set(geo, "sop", Value::Str("/sphere1".into()));
    rig.set(cam, "translate", Value::Vec3([0.0, 0.0, 4.0]));
    rig.set(render, "camera", Value::Str("/cam1".into()));
    rig.set(render, "light", Value::Str("/light1".into()));
    rig.set(render, "resw", Value::Int(160));
    rig.set(render, "resh", Value::Int(160));
    render
}

#[test]
fn a_render_top_draws_geometry() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let render = basic_scene(&mut rig);
    rig.run(render);

    let (w, h, px) = rig.pixels(render);
    assert_eq!((w, h), (160, 160));

    // Something in the middle, nothing in the corner: a sphere on a black
    // background is the simplest thing that proves the whole pipeline.
    let at = |x: usize, y: usize| -> [u8; 4] {
        let i = (y * w as usize + x) * 4;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    };
    let centre = at(80, 80);
    assert!(
        centre[0] as u32 + centre[1] as u32 + centre[2] as u32 > 60,
        "nothing drawn in the middle: {centre:?}"
    );
    assert_eq!(at(2, 2)[0], 0, "the background should be clear");
    let _ = h;
}

#[test]
fn the_camera_actually_moves_the_view() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let render = basic_scene(&mut rig);
    let cam = rig.graph.find("/cam1").unwrap();

    rig.run(render);
    let near = rig.lit(render);

    // Backing the camera off makes the sphere smaller.
    rig.set(cam, "translate", Value::Vec3([0.0, 0.0, 12.0]));
    rig.run(render);
    let far = rig.lit(render);

    assert!(near > 0 && far > 0, "{near} then {far}");
    assert!(
        near > far * 3,
        "moving the camera away should shrink it: {near} then {far}"
    );
}

#[test]
fn depth_testing_puts_the_nearer_object_in_front() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let render = basic_scene(&mut rig);

    // A second, larger sphere behind the first, in a different colour.
    let far_sop = rig.add("sphereSOP", "sphere2");
    let far_geo = rig.add("geometryCOMP", "geo2");
    let far_mat = rig.add("pbrMAT", "mat_far");
    rig.set(far_sop, "radius", Value::Float(1.5));
    rig.set(far_geo, "sop", Value::Str("/sphere2".into()));
    rig.set(far_geo, "material", Value::Str("/mat_far".into()));
    rig.set(far_geo, "translate", Value::Vec3([0.0, 0.0, -2.0]));
    rig.set(far_mat, "basecolor", Value::Vec4([1.0, 0.0, 0.0, 1.0]));
    rig.set(far_mat, "emit", Value::Float(2.0));

    let near_mat = rig.add("pbrMAT", "mat_near");
    rig.set(near_mat, "basecolor", Value::Vec4([0.0, 0.0, 1.0, 1.0]));
    rig.set(near_mat, "emit", Value::Float(2.0));
    let near_geo = rig.graph.find("/geo1").unwrap();
    rig.set(near_geo, "material", Value::Str("/mat_near".into()));

    rig.run(render);
    let (w, _, px) = rig.pixels(render);
    let i = (80 * w as usize + 80) * 4;
    let centre = [px[i], px[i + 1], px[i + 2]];
    assert!(
        centre[2] > centre[0],
        "the nearer blue sphere should win the depth test: {centre:?}"
    );
}

#[test]
fn a_material_changes_what_is_drawn() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let render = basic_scene(&mut rig);
    let mat = rig.add("pbrMAT", "mat1");
    let geo = rig.graph.find("/geo1").unwrap();
    rig.set(geo, "material", Value::Str("/mat1".into()));
    rig.set(mat, "basecolor", Value::Vec4([1.0, 0.0, 0.0, 1.0]));
    rig.run(render);

    let (w, _, px) = rig.pixels(render);
    let i = (80 * w as usize + 80) * 4;
    assert!(px[i] > px[i + 2] + 40, "expected red: {:?}", &px[i..i + 3]);
}

#[test]
fn instancing_draws_one_geometry_many_times() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let render = basic_scene(&mut rig);
    let geo = rig.graph.find("/geo1").unwrap();
    let sphere = rig.graph.find("/sphere1").unwrap();
    rig.set(sphere, "radius", Value::Float(0.15));

    // A Pattern CHOP's samples become instance positions — one channel of
    // 12 samples is 12 spheres.
    let pattern = rig.add("patternCHOP", "positions");
    rig.set(pattern, "type", Value::Str("sine".into()));
    rig.set(pattern, "length", Value::Int(12));
    rig.set(pattern, "amplitude", Value::Float(1.5));
    rig.set(pattern, "name", Value::Str("tx".into()));

    rig.set(geo, "instancing", Value::Bool(true));
    rig.set(geo, "instancechop", Value::Str("/positions".into()));
    rig.set(geo, "ty", Value::Str("".into()));
    rig.set(geo, "tz", Value::Str("".into()));

    rig.run(render);
    let (draws, instances) = rig.engines.top.render_stats();
    assert_eq!(draws, 1, "one draw call");
    assert_eq!(instances, 12, "twelve instances");

    // And the picture is wider than a single sphere at the origin.
    let (w, _, px) = rig.pixels(render);
    let lit_columns = (0..w as usize)
        .filter(|x| {
            (0..w as usize).any(|y| {
                let i = (y * w as usize + x) * 4;
                px[i] as u32 + px[i + 1] as u32 + px[i + 2] as u32 > 30
            })
        })
        .count();
    assert!(
        lit_columns > 60,
        "instances should spread out: {lit_columns}"
    );
}

#[test]
fn instancing_follows_a_channel_that_changes() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let render = basic_scene(&mut rig);
    let geo = rig.graph.find("/geo1").unwrap();
    rig.set(
        rig.graph.find("/sphere1").unwrap(),
        "radius",
        Value::Float(0.2),
    );

    let pattern = rig.add("patternCHOP", "positions");
    rig.set(pattern, "length", Value::Int(8));
    rig.set(pattern, "name", Value::Str("tx".into()));
    rig.set(pattern, "amplitude", Value::Float(0.5));
    rig.set(geo, "instancing", Value::Bool(true));
    rig.set(geo, "instancechop", Value::Str("/positions".into()));
    rig.set(geo, "ty", Value::Str("".into()));
    rig.set(geo, "tz", Value::Str("".into()));

    rig.run(render);
    let tight = rig.lit(render);

    // Spreading the same instances out changes the image, which is the whole
    // point of driving them from the network.
    rig.set(pattern, "amplitude", Value::Float(2.5));
    rig.run(render);
    let spread = rig.lit(render);
    assert_ne!(
        tight, spread,
        "the instance positions did not reach the GPU"
    );
}

#[test]
fn a_render_top_with_nothing_in_it_still_produces_a_texture() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let render = rig.add("renderTOP", "render1");
    rig.set(render, "resw", Value::Int(64));
    rig.set(render, "resh", Value::Int(64));
    rig.set(render, "background", Value::Vec4([0.0, 0.25, 0.0, 1.0]));
    rig.run(render);

    let (_, _, px) = rig.pixels(render);
    // The background clear still happened, so downstream TOPs have something.
    assert!((px[1] as i32 - 64).abs() < 4, "background: {:?}", &px[0..4]);
}

#[test]
fn a_render_top_feeds_a_top_chain() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let render = basic_scene(&mut rig);
    let level = rig.add("levelTOP", "level1");
    rig.graph.connect(render, level, 0).unwrap();
    rig.set(level, "invert", Value::Float(1.0));
    rig.run(level);

    // Inverting a mostly-black render gives a mostly-white image: the 3D
    // output is an ordinary texture from here on.
    let tex = rig.engines.top.output(&rig.graph, level).unwrap().clone();
    let (_, _, px) = read_pixels_rgba8(&rig.gpu, &tex).unwrap();
    assert!(px[0] > 200, "corner should be inverted to white: {}", px[0]);
}

#[test]
fn the_scene_references_are_cook_dependencies() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let render = basic_scene(&mut rig);
    rig.run(render);

    // Pulling only the Render TOP must have cooked the geometry it names.
    let sphere = rig.graph.find("/sphere1").unwrap();
    assert!(
        rig.cook.cook_count(sphere) > 0,
        "the SOP should have been dragged in"
    );
}

// ---------------------------------------------------------------- materials

/// The brightest pixel found on a horizontal scan through the middle.
fn peak_on_centre_row(rig: &Rig, render: NodeId) -> [u8; 3] {
    let (w, h, px) = rig.pixels(render);
    let y = h as usize / 2;
    let mut best = [0u8; 3];
    for x in 0..w as usize {
        let i = (y * w as usize + x) * 4;
        if px[i] as u32 + px[i + 1] as u32 + px[i + 2] as u32
            > best[0] as u32 + best[1] as u32 + best[2] as u32
        {
            best = [px[i], px[i + 1], px[i + 2]];
        }
    }
    best
}

#[test]
fn a_constant_material_ignores_the_light() {
    // The point of the operator: turn the light off entirely and the surface
    // must be unchanged. A PBR material in the same place must not be.
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let render = basic_scene(&mut rig);
    let geo = rig.graph.find("/geo1").unwrap();
    let light = rig.graph.find("/light1").unwrap();
    let mat = rig.add("constantMAT", "flat1");
    rig.set(geo, "material", Value::Str("/flat1".into()));
    rig.set(mat, "basecolor", Value::Vec4([0.0, 1.0, 0.0, 1.0]));

    rig.run(render);
    let lit = peak_on_centre_row(&rig, render);
    assert!(lit[1] > 100, "the sphere should be green: {lit:?}");

    rig.set(light, "intensity", Value::Float(0.0));
    rig.run(render);
    let unlit = peak_on_centre_row(&rig, render);
    assert_eq!(lit, unlit, "a constant material must not notice the light");
}

#[test]
fn a_phong_materials_shininess_tightens_the_highlight() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let render = basic_scene(&mut rig);
    let geo = rig.graph.find("/geo1").unwrap();
    let mat = rig.add("phongMAT", "phong1");
    rig.set(geo, "material", Value::Str("/phong1".into()));
    rig.set(mat, "basecolor", Value::Vec4([0.1, 0.1, 0.1, 1.0]));
    rig.set(mat, "specular", Value::Float(1.0));

    // Count the pixels the highlight covers, wide then narrow.
    let hot = |rig: &Rig| -> usize {
        let (w, h, px) = rig.pixels(render);
        (0..(w as usize * h as usize))
            .filter(|i| px[i * 4] > 180)
            .count()
    };

    rig.set(mat, "shininess", Value::Float(4.0));
    rig.run(render);
    let broad = hot(&rig);

    rig.set(mat, "shininess", Value::Float(200.0));
    rig.run(render);
    let tight = hot(&rig);

    assert!(broad > 0, "a specular of 1.0 should show a highlight");
    assert!(
        tight < broad,
        "shininess 200 should be tighter than 4: {tight} vs {broad}"
    );
}

#[test]
fn a_wireframe_material_draws_edges_and_leaves_holes() {
    // A wireframe is mostly background — that is what distinguishes it from a
    // filled surface, and it is the cheapest thing to assert that a filled
    // draw could not accidentally pass.
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let render = basic_scene(&mut rig);
    let geo = rig.graph.find("/geo1").unwrap();
    let sphere = rig.graph.find("/sphere1").unwrap();
    rig.set(sphere, "radius", Value::Float(1.2));
    // A coarse mesh on purpose: at the default 16x24 the edges alone cover
    // half the sphere and the test could not tell a wireframe from a fill.
    rig.set(sphere, "rows", Value::Int(5));
    rig.set(sphere, "columns", Value::Int(7));

    let covered = |rig: &Rig| -> usize {
        let (w, h, px) = rig.pixels(render);
        (0..(w as usize * h as usize))
            .filter(|i| px[i * 4] as u32 + px[i * 4 + 1] as u32 + px[i * 4 + 2] as u32 > 30)
            .count()
    };

    rig.run(render);
    let solid = covered(&rig);

    rig.add("wireframeMAT", "wire1");
    rig.set(geo, "material", Value::Str("/wire1".into()));
    rig.run(render);
    let wire = covered(&rig);

    assert!(solid > 0, "the solid sphere should cover something");
    assert!(
        wire * 2 < solid,
        "a wireframe should be mostly holes: {wire} vs {solid}"
    );
    assert!(wire > 0, "but it should still draw its edges");
}
