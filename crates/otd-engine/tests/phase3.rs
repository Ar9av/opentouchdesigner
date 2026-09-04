//! The Phase 3 exit criterion from PLAN.md:
//!
//!   "a reusable audio-visualizer component instantiated twice with different
//!    parameters; project diffed meaningfully in git"
//!
//! Both halves are tested: the component behaves differently in each instance
//! while sharing one definition, and changing one knob produces a diff you
//! could review.

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

fn run(graph: &Graph, engines: &mut Engines, cook: &mut CookEngine, root: NodeId, frames: usize) {
    let mut time = CookContext::default();
    for _ in 0..frames {
        engines.begin_frame();
        cook.cook_frame(graph, &[root], &time, engines).unwrap();
        engines.end_frame();
        time.advance(1.0 / 60.0);
    }
}

#[test]
fn one_component_used_twice_behaves_differently_in_each_place() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let (graph, out) = demo::two_visualisers(&reg);
    let mut engines = Engines::new(gpu.clone());
    if engines.python_error().is_some() {
        eprintln!("skipping: no interpreter");
        return;
    }
    let mut cook = CookEngine::new();
    run(&graph, &mut engines, &mut cook, out, 4);

    for id in graph.walk() {
        assert!(
            engines.node_status(&graph, id).is_none()
                // A device that is not plugged in is expected here.
                || graph.node(id).op_type.contains("audiodevice"),
            "{}: {:?}",
            graph.path(id),
            engines.node_status(&graph, id)
        );
    }

    // Both instances rendered, and they are not the same picture: the same
    // network, told to be a different colour.
    let read = |path: &str| {
        let id = graph.find(path).unwrap();
        let tex = engines
            .top
            .output(&graph, id)
            .unwrap_or_else(|| panic!("{path} produced nothing"))
            .clone();
        read_pixels_rgba8(&gpu, &tex).unwrap().2
    };
    let bass = read("/bass_vis");
    let treble = read("/treble_vis");
    assert_ne!(bass, treble, "two instances rendered identically");

    // The tint really did differ: one is warm, the other cool.
    let sum = |px: &[u8], offset: usize| -> u64 {
        px.iter().skip(offset).step_by(4).map(|v| *v as u64).sum()
    };
    assert!(
        sum(&bass, 0) > sum(&bass, 2),
        "the bass instance should be warm"
    );
    assert!(
        sum(&treble, 2) > sum(&treble, 0),
        "the treble instance should be cool"
    );
}

#[test]
fn the_two_instances_share_one_definition() {
    let reg = registry();
    let (graph, _) = demo::two_visualisers(&reg);

    // Same structure inside each.
    for name in ["bass_vis", "treble_vis"] {
        for child in ["in1", "pick", "smooth", "ring", "out1"] {
            assert!(
                graph.find(&format!("/{name}/{child}")).is_some(),
                "/{name}/{child} missing"
            );
        }
    }
    // Same shader source, byte for byte — the visual is not duplicated code.
    let source_of = |path: &str| {
        graph
            .node(graph.find(path).unwrap())
            .param("source")
            .unwrap()
            .value
            .as_str()
    };
    assert_eq!(source_of("/bass_vis/ring"), source_of("/treble_vis/ring"));

    // And different settings on the outside.
    let par = |path: &str, key: &str| {
        graph
            .node(graph.find(path).unwrap())
            .param(key)
            .unwrap()
            .value
            .clone()
    };
    assert_eq!(par("/bass_vis", "band"), Value::Int(1));
    assert_eq!(par("/treble_vis", "band"), Value::Int(4));
    assert_ne!(par("/bass_vis", "hue"), par("/treble_vis", "hue"));
}

#[test]
fn turning_one_knob_produces_a_reviewable_diff() {
    let reg = registry();
    let (mut graph, _) = demo::two_visualisers(&reg);
    let before = Project::from_graph(&graph, &reg, 60.0).to_ron().unwrap();

    let treble = graph.find("/treble_vis").unwrap();
    graph.set_param(treble, "hue", Value::Float(0.8)).unwrap();
    let after = Project::from_graph(&graph, &reg, 60.0).to_ron().unwrap();

    // A one-knob change is a one-line change. This is the property PLAN.md
    // §1 calls a headline feature, so it is worth asserting rather than
    // hoping for.
    let changed: Vec<(&str, &str)> = before
        .lines()
        .zip(after.lines())
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(
        before.lines().count(),
        after.lines().count(),
        "the file should not change shape"
    );
    assert_eq!(
        changed.len(),
        1,
        "expected one changed line, got {changed:?}"
    );
    assert!(changed[0].0.contains("0.55"), "{:?}", changed[0]);
    assert!(changed[0].1.contains("0.8"), "{:?}", changed[0]);
}

#[test]
fn the_whole_patch_round_trips_including_components_and_tables() {
    let reg = registry();
    let (graph, _) = demo::two_visualisers(&reg);
    let text = Project::from_graph(&graph, &reg, 60.0).to_ron().unwrap();

    // The cue table's contents are in the file, not in a side car.
    assert!(text.contains("drop"), "the cue table should be in the file");
    // So are the component's custom parameters, in full.
    assert!(text.contains("custom: true"));

    let back = Project::from_ron(&text).unwrap().to_graph(&reg).unwrap();
    assert_eq!(graph.len(), back.len());
    assert_eq!(
        back.node(back.find("/bass_vis").unwrap()).input_labels,
        vec!["in1"],
        "the component's connectors were rebuilt from its In operators"
    );
    let text2 = Project::from_graph(&back, &reg, 60.0).to_ron().unwrap();
    assert_eq!(text, text2, "second round trip must be byte-identical");
}

#[test]
fn adding_a_third_instance_appends_rather_than_rewrites() {
    let reg = registry();
    let (mut graph, _) = demo::two_visualisers(&reg);
    let before = Project::from_graph(&graph, &reg, 60.0).to_ron().unwrap();

    let root = graph.root();
    demo::visualiser(&reg, &mut graph, root, "mid_vis");
    let after = Project::from_graph(&graph, &reg, 60.0).to_ron().unwrap();

    // Every line of the original survives in order: adding a component adds
    // blocks, it does not reflow the file.
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();
    let mut cursor = 0;
    for line in &before_lines {
        match after_lines[cursor..].iter().position(|l| l == line) {
            Some(offset) => cursor += offset + 1,
            None => panic!("adding a component rewrote an existing line: {line}"),
        }
    }
    assert!(after_lines.len() > before_lines.len());
}
