//! The palette components, proven to do their jobs — not merely to build.
//!
//! A palette entry that compiles but renders nothing would be worse than no
//! palette: it is the first thing a new user drops into a patch.

use otd_core::{CookContext, CookEngine, Graph, NodeId, Value};
use otd_engine::{Engines, palette, registry};
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
    engines: Engines,
    cook: CookEngine,
    time: CookContext,
    gpu: GpuContext,
}

impl Rig {
    fn new(gpu: GpuContext) -> Self {
        Rig {
            graph: Graph::new(),
            engines: Engines::new(gpu.clone()),
            cook: CookEngine::new(),
            time: CookContext::default(),
            gpu,
        }
    }

    fn run(&mut self, root: NodeId, frames: usize) {
        for _ in 0..frames {
            self.engines.begin_frame();
            self.cook
                .cook_frame(&self.graph, &[root], &self.time, &mut self.engines)
                .unwrap();
            self.engines.end_frame();
            self.time.advance(1.0 / 60.0);
        }
    }

    fn brightness(&self, id: NodeId) -> f64 {
        let tex = self
            .engines
            .top
            .output(&self.graph, id)
            .expect("the TOP has cooked")
            .clone();
        let (_, _, pixels) = read_pixels_rgba8(&self.gpu, &tex).unwrap();
        let sum: u64 = pixels.iter().step_by(4).map(|p| *p as u64).sum();
        sum as f64 / (pixels.len() / 4) as f64
    }

    fn pixels(&self, id: NodeId) -> Vec<u8> {
        let tex = self
            .engines
            .top
            .output(&self.graph, id)
            .expect("the TOP has cooked")
            .clone();
        let (_, _, pixels) = read_pixels_rgba8(&self.gpu, &tex).unwrap();
        pixels
    }

    fn corner_and_centre(&self, id: NodeId) -> (f64, f64) {
        let tex = self
            .engines
            .top
            .output(&self.graph, id)
            .expect("the TOP has cooked")
            .clone();
        let (w, h, pixels) = read_pixels_rgba8(&self.gpu, &tex).unwrap();
        let at = |x: usize, y: usize| pixels[(y * w as usize + x) * 4] as f64;
        (at(1, 1), at(w as usize / 2, h as usize / 2))
    }
}

/// A source, a palette effect wired onto it, and a null to look at.
fn effect_rig(rig: &mut Rig, item: &str) -> NodeId {
    let reg = registry();
    let root = rig.graph.root();
    let src = rig
        .graph
        .create(root, reg.get("constantTOP").unwrap(), Some("src"))
        .unwrap();
    rig.graph
        .set_param(src, "color", Value::Vec4([0.5, 0.5, 0.5, 1.0]))
        .unwrap();
    let comp = palette::find(item)
        .expect("palette item exists")
        .build(&mut rig.graph, &reg, root);
    let out = rig
        .graph
        .create(root, reg.get("nullTOP").unwrap(), Some("look"))
        .unwrap();
    rig.graph.connect(src, comp, 0).unwrap();
    rig.graph.connect(comp, out, 0).unwrap();
    out
}

#[test]
fn every_item_builds_wires_and_cooks() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    for item in palette::ITEMS {
        let mut rig = Rig::new(gpu.clone());
        let root = rig.graph.root();
        let comp = item.build(&mut rig.graph, &reg, root);
        // The API surface a user meets: connectors from the In/Out ops,
        // custom parameters as the knobs.
        let node = rig.graph.node(comp);
        assert!(
            node.params.values().any(|p| p.custom),
            "{}: a palette component with no knobs is just a group",
            item.name
        );
        assert!(
            rig.graph.output_family(comp).is_some(),
            "{}: nothing to present",
            item.name
        );
        assert!(!item.summary.is_empty());
        // And it cooks bare — with nothing wired in — without failing.
        rig.run(comp, 2);
    }
}

#[test]
fn trails_move_because_the_relative_feedback_target_resolves() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut rig = Rig::new(gpu);
    let root = rig.graph.root();

    // A ramp, not a flat colour: trails spread the bright region, and
    // spreading is invisible on a uniform image.
    let src = rig
        .graph
        .create(root, reg.get("rampTOP").unwrap(), Some("src"))
        .unwrap();
    let comp = palette::find("trails")
        .unwrap()
        .build(&mut rig.graph, &reg, root);
    let out = rig
        .graph
        .create(root, reg.get("nullTOP").unwrap(), Some("look"))
        .unwrap();
    rig.graph.connect(src, comp, 0).unwrap();
    rig.graph.connect(comp, out, 0).unwrap();

    // Turn the drift up so what the loop does is unmistakable.
    rig.graph
        .set_param(comp, "rotate", Value::Float(15.0))
        .unwrap();

    // The real assertion is that the loop is *alive*, which needs `out1` to
    // resolve relative to the instance. The source is static, so with a dead
    // loop every frame is identical — only feedback can make the picture
    // move.
    rig.run(out, 3);
    let a = rig.pixels(out);
    rig.run(out, 20);
    let b = rig.pixels(out);
    let moved = a
        .iter()
        .zip(&b)
        .filter(|(x, y)| (**x as i32 - **y as i32).abs() > 8)
        .count();
    assert!(
        moved > a.len() / 100,
        "a static source but a live loop must move the picture; {moved} of {} pixels changed",
        a.len()
    );
}

#[test]
fn bloom_adds_light_and_vignette_takes_it_from_the_corners() {
    let gpu = gpu_or_skip!();

    let mut rig = Rig::new(gpu.clone());
    let out = effect_rig(&mut rig, "bloom");
    rig.run(out, 3);
    let with_bloom = rig.brightness(out);
    assert!(
        with_bloom > 0.5 * 255.0 * 1.2,
        "bloom should brighten a mid-grey: {with_bloom:.1}"
    );

    let mut rig = Rig::new(gpu);
    let out = effect_rig(&mut rig, "vignette");
    rig.run(out, 3);
    let (corner, centre) = rig.corner_and_centre(out);
    assert!(
        corner < centre * 0.7,
        "corners should darken: corner={corner:.1} centre={centre:.1}"
    );
}

#[test]
fn audio_level_presents_one_channel_with_no_device_needed() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut rig = Rig::new(gpu);
    let root = rig.graph.root();
    let comp = palette::find("audiolevel")
        .unwrap()
        .build(&mut rig.graph, &reg, root);
    rig.run(comp, 3);
    // No microphone on CI: the channel exists and reads silence, because a
    // missing device must degrade, not fail.
    let value = rig
        .engines
        .channel_value(&rig.graph, &rig.graph.path(comp), "band1");
    assert_eq!(value, Some(0.0));
}
