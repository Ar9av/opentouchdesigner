//! The Text TOP.
//!
//! Its unit tests in `otd-gpu` cover the layout; these cover the part that
//! only exists once the operator is in a network — that the glyphs reach the
//! GPU, that the colour is a live parameter rather than baked into the raster,
//! and that a machine with no font says so instead of going quietly black.

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
    fn pixels(&self, id: NodeId) -> Vec<u8> {
        let tex = self.engines.top.output(&self.graph, id).unwrap().clone();
        read_pixels_rgba8(&self.gpu, &tex).unwrap().2
    }
}

/// Skip rather than fail where there is no font to draw with — the same
/// bargain the GPU tests make.
fn has_a_font(rig: &Rig, id: NodeId) -> bool {
    match rig.engines.node_status(&rig.graph, id) {
        Some(status) if status.contains("no system font") => {
            eprintln!("skipping: {status}");
            false
        }
        _ => true,
    }
}

fn text_node(rig: &mut Rig) -> NodeId {
    let t = rig.add("textTOP", "caption");
    rig.set(t, "resw", Value::Int(256));
    rig.set(t, "resh", Value::Int(128));
    rig.set(t, "size", Value::Float(48.0));
    rig.set(t, "text", Value::Str("Hello".into()));
    t
}

#[test]
fn text_reaches_the_gpu_as_a_picture() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let t = text_node(&mut rig);
    rig.run(t);
    if !has_a_font(&rig, t) {
        return;
    }

    let px = rig.pixels(t);
    let ink = px.chunks(4).filter(|p| p[3] > 8).count();
    assert!(ink > 50, "expected glyphs on the texture, got {ink} pixels");

    // Centred by default, so the corners are empty and the middle is not.
    let at = |x: usize, y: usize| px[(y * 256 + x) * 4 + 3];
    assert_eq!(at(2, 2), 0, "the corner should be transparent");
}

#[test]
fn the_colour_is_a_live_parameter_and_not_baked_into_the_raster() {
    // The whole reason the glyphs are uploaded as coverage and tinted in the
    // shader: turning the colour knob must not re-lay-out the text.
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let t = text_node(&mut rig);
    rig.set(t, "color", Value::Vec4([1.0, 0.0, 0.0, 1.0]));
    rig.run(t);
    if !has_a_font(&rig, t) {
        return;
    }

    let red = rig.pixels(t);
    let red_ink: Vec<usize> = red
        .chunks(4)
        .enumerate()
        .filter(|(_, p)| p[3] > 128)
        .map(|(i, _)| i)
        .collect();
    assert!(!red_ink.is_empty());
    assert!(
        red[red_ink[0] * 4] > red[red_ink[0] * 4 + 2],
        "should be red"
    );

    rig.set(t, "color", Value::Vec4([0.0, 0.0, 1.0, 1.0]));
    rig.run(t);
    let blue = rig.pixels(t);

    // Same pixels covered, different colour in them.
    let blue_ink: Vec<usize> = blue
        .chunks(4)
        .enumerate()
        .filter(|(_, p)| p[3] > 128)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(red_ink, blue_ink, "the glyphs must not have moved");
    assert!(
        blue[blue_ink[0] * 4 + 2] > blue[blue_ink[0] * 4],
        "should be blue now"
    );
}

#[test]
fn changing_the_text_changes_the_picture() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let t = text_node(&mut rig);
    rig.set(t, "text", Value::Str("i".into()));
    rig.run(t);
    if !has_a_font(&rig, t) {
        return;
    }
    let narrow = rig.pixels(t).chunks(4).filter(|p| p[3] > 8).count();

    rig.set(t, "text", Value::Str("wwwwwwww".into()));
    rig.run(t);
    let wide = rig.pixels(t).chunks(4).filter(|p| p[3] > 8).count();

    assert!(
        wide > narrow * 4,
        "eight w's should cover far more than one i: {wide} vs {narrow}"
    );
}

#[test]
fn a_missing_font_is_reported_rather_than_failing_the_cook() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let t = text_node(&mut rig);
    rig.set(t, "font", Value::Str("/no/such/font/anywhere.ttf".into()));
    rig.run(t);

    let status = rig
        .engines
        .node_status(&rig.graph, t)
        .expect("the node should say the font is missing");
    assert!(status.contains("anywhere.ttf"), "unhelpful: {status}");
    // And the chain keeps running: a texture exists, it is simply empty.
    assert!(rig.engines.top.output(&rig.graph, t).is_some());
}

#[test]
fn text_composites_over_another_top_like_any_other_texture() {
    // The point of the alpha being real: a caption is something you put on
    // top of a picture, and that has to work with the ordinary Composite.
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let bg = rig.add("constantTOP", "bg");
    let t = text_node(&mut rig);
    let over = rig.add("compositeTOP", "over1");
    rig.set(bg, "resw", Value::Int(256));
    rig.set(bg, "resh", Value::Int(128));
    rig.set(bg, "color", Value::Vec4([0.0, 0.4, 0.0, 1.0]));
    rig.graph.connect(bg, over, 0).unwrap();
    rig.graph.connect(t, over, 1).unwrap();
    rig.run(over);
    if !has_a_font(&rig, t) {
        return;
    }

    let px = rig.pixels(over);
    // The background survives where there is no glyph.
    let corner = &px[0..4];
    assert!(corner[1] > 80 && corner[0] < 40, "background: {corner:?}");
    // And somewhere the text has brightened it.
    assert!(
        px.chunks(4).any(|p| p[0] > 200 && p[2] > 200),
        "expected white glyphs over the green"
    );
}
