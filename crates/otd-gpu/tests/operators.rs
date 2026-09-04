//! Every built-in TOP cooks.
//!
//! WGSL is compiled when the pipeline is built, not when the crate is, so a
//! shader with a typo in it compiles clean and then fails the first time
//! somebody drops the node. Adding an operator is meant to be one `.wgsl`
//! file and one table entry (see the module docs in `ops.rs`) — this is the
//! test that keeps that cheap without letting it get sloppy.

use otd_core::{CookContext, CookEngine, Graph, NodeId};
use otd_gpu::{GpuContext, TopEngine, ops};

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

#[test]
fn every_top_compiles_its_shader_and_produces_a_texture() {
    let ctx = gpu_or_skip!();
    let registry = ops::registry();
    let mut engine = TopEngine::new(ctx);
    let time = CookContext::default();

    for spec in ops::all() {
        // A fresh cook engine per operator. Slotmap keys restart with each
        // graph, so a shared one would see the second operator's node as the
        // first operator's node, already cooked and clean, and skip it.
        let mut cook = CookEngine::new();
        // Operators the engine intercepts before any shader runs, each for a
        // reason the rest of this suite covers: a decoder thread, a 3D
        // pipeline, a wire of another family, a component boundary.
        if matches!(
            spec.def.type_name,
            ops::RENDER | ops::MOVIE_IN | ops::VIDEO_DEVICE_IN | ops::CHOP_TO_TOP | ops::IN
        ) {
            continue;
        }

        let mut graph = Graph::new();
        let root = graph.root();
        // A source to feed the filters, so an operator that samples input 0
        // is not tested against the 1x1 dummy.
        let src = graph
            .create(root, registry.get("noiseTOP").unwrap(), Some("src"))
            .unwrap();
        let node = graph
            .create(root, &spec.def.clone(), Some("subject"))
            .unwrap();
        for i in 0..spec.def.inputs.len() {
            let _ = graph.connect(src, node, i);
        }

        engine.begin_frame();
        let result = cook.cook_frame(&graph, &[node], &time, &mut engine);
        engine.end_frame();

        let name = spec.def.type_name;
        assert!(result.is_ok(), "{name} failed to cook: {result:?}");
        assert!(
            engine.shader_error(node).is_none(),
            "{name} shader: {:?}",
            engine.shader_error(node)
        );
        assert!(
            engine.output(&graph, node).is_some(),
            "{name} produced no texture"
        );
        engine.forget(node);
        engine.forget(src);
    }
}

/// A generator sized from its own parameters must honour them, and a filter
/// must inherit from its input. Getting this backwards is the mistake a new
/// `Sizing` entry invites, and it is invisible until something downstream is
/// the wrong shape.
#[test]
fn sizing_follows_what_the_operator_declared() {
    let ctx = gpu_or_skip!();
    let registry = ops::registry();
    let mut engine = TopEngine::new(ctx);
    let mut cook = CookEngine::new();
    let time = CookContext::default();

    let mut graph = Graph::new();
    let root = graph.root();
    let circle = graph
        .create(root, registry.get("circleTOP").unwrap(), Some("disc"))
        .unwrap();
    graph
        .set_param(circle, "resw", otd_core::Value::Int(300))
        .unwrap();
    graph
        .set_param(circle, "resh", otd_core::Value::Int(200))
        .unwrap();
    let edge = graph
        .create(root, registry.get("edgeTOP").unwrap(), Some("outline"))
        .unwrap();
    graph.connect(circle, edge, 0).unwrap();

    let mut run = |id: NodeId| {
        engine.begin_frame();
        cook.cook_frame(&graph, &[id], &time, &mut engine).unwrap();
        engine.end_frame();
    };
    run(edge);

    let c = engine.output(&graph, circle).unwrap();
    assert_eq!((c.key.width, c.key.height), (300, 200));
    let e = engine.output(&graph, edge).unwrap();
    assert_eq!((e.key.width, e.key.height), (300, 200));
}
