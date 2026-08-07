//! Keyframes, end to end.
//!
//! The curve maths is tested in `otd-chop`. What matters here is the claim the
//! design rests on: because keyframes are an ordinary CHOP, they reach a
//! parameter through the ordinary Export and expression paths and end up in
//! pixels with no new mechanism anywhere.

use otd_chop::anim::Curves;
use otd_core::{CookContext, CookEngine, Graph, NodeId, Project, Value};
use otd_engine::{Engines, demo, registry};
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
    fn new(gpu: GpuContext, graph: Graph) -> Self {
        Rig {
            graph,
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

    fn channel(&self, path: &str, name: &str) -> f32 {
        self.engines
            .channel_value(&self.graph, path, name)
            .unwrap_or_else(|| panic!("no channel {path}/{name}"))
    }
}

#[test]
fn keyframes_reach_the_pixels() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let (graph, out) = demo::keyframes(&reg);
    let mut rig = Rig::new(gpu, graph);

    // `bright` is keyed 0.5 at t=0, stepping to 2.0 at t=1 and holding to
    // t=3. Sampling either side of the step has to give visibly different
    // images, or the curve is not reaching the render.
    rig.run(out, 6);
    let dim = rig.brightness(out);
    rig.run(out, 90);
    let bright = rig.brightness(out);

    assert!(
        bright > dim * 1.5,
        "the keyed curve did not reach the texture: {dim:.1} then {bright:.1}"
    );
}

#[test]
fn the_curve_is_read_at_the_value_it_was_keyed_to() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let (graph, out) = demo::keyframes(&reg);
    let mut rig = Rig::new(gpu, graph);

    // A constant-interpolated segment holds exactly, so this is an equality
    // rather than a "roughly" — anything else means the sampler is drifting.
    rig.run(out, 120); // t = 2.0, inside the 1..3 hold
    assert!(
        (rig.channel("/anim1", "bright") - 2.0).abs() < 1e-4,
        "{}",
        rig.channel("/anim1", "bright")
    );

    // And `rotate` is smooth from 0 to 180 across four seconds: halfway
    // through, an ease is exactly at the midpoint.
    assert!(
        (rig.channel("/anim1", "rotate") - 90.0).abs() < 2.0,
        "{}",
        rig.channel("/anim1", "rotate")
    );
}

#[test]
fn an_animation_chop_is_time_dependent_so_the_chain_keeps_cooking() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let (graph, out) = demo::keyframes(&reg);
    let mut rig = Rig::new(gpu, graph);
    rig.run(out, 2);

    // The failure this guards against is the one every stateful CHOP hit in
    // Phase 2: an operator that is not marked time dependent cooks once, and
    // the animation freezes on frame one while everything looks fine.
    let anim = rig.graph.find("/anim1").unwrap();
    let xform = rig.graph.find("/xform1").unwrap();
    assert!(rig.cook.is_time_dependent(anim));
    assert!(
        rig.cook.is_time_dependent(xform),
        "time dependence has to propagate through the export to the TOP"
    );
}

#[test]
fn keys_survive_a_save_and_load() {
    let reg = registry();
    let (graph, _) = demo::keyframes(&reg);

    // Keyframes are text in the project file precisely so this works — and so
    // the diff of a re-keyed animation is the lines that changed.
    let text = Project::from_graph(&graph, &reg, 60.0).to_ron().unwrap();
    let loaded = Project::from_ron(&text).unwrap().to_graph(&reg).unwrap();

    let id = loaded.find("/anim1").unwrap();
    let keys = loaded.node(id).param("keys").unwrap().value.as_str();
    let (curves, problems) = Curves::parse(&keys);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(curves.0.len(), 3);
    assert_eq!(curves.0["rotate"].sample(4.0), 180.0);
    assert_eq!(curves.0["bright"].sample(2.0), 2.0);
}

#[test]
fn editing_one_key_is_a_one_line_diff() {
    let reg = registry();
    let (mut graph, _) = demo::keyframes(&reg);
    let before = Project::from_graph(&graph, &reg, 60.0).to_ron().unwrap();

    let id = graph.find("/anim1").unwrap();
    let (mut curves, _) = Curves::parse(&graph.node(id).param("keys").unwrap().value.as_str());
    curves
        .0
        .get_mut("rotate")
        .unwrap()
        .set(4.0, 360.0, otd_chop::anim::Interp::Smooth);
    graph
        .set_param(id, "keys", Value::Str(curves.to_text()))
        .unwrap();

    let after = Project::from_graph(&graph, &reg, 60.0).to_ron().unwrap();
    let changed = before
        .lines()
        .zip(after.lines())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        before.lines().count(),
        after.lines().count(),
        "moving a key must not reflow the file"
    );
    assert_eq!(changed, 1, "one key moved should be one changed line");
}
