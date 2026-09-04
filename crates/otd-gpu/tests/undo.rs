//! Undo against the live engine.
//!
//! The history module's own tests prove a snapshot restores the graph. What
//! they cannot prove is the thing that would actually bite: the cook engine
//! caches by node id *and revision*, and an undo hands it a graph whose
//! revision numbers have gone backwards. If that made a cached texture look
//! current, undo would change the parameter panel and not the picture — which
//! is the worst possible failure, because it looks like it worked.

use otd_core::{CookContext, CookEngine, Graph, History, Value};
use otd_gpu::{GpuContext, TopEngine, ops, read_pixels_rgba8};

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
fn undo_changes_the_pixels_not_just_the_parameter() {
    let ctx = gpu_or_skip!();
    let registry = ops::registry();

    let mut graph = Graph::new();
    let root = graph.root();
    let node = graph
        .create(root, registry.get("constantTOP").unwrap(), Some("c1"))
        .unwrap();
    graph.set_param(node, "resw", Value::Int(32)).unwrap();
    graph.set_param(node, "resh", Value::Int(32)).unwrap();
    graph
        .set_param(node, "color", Value::Vec4([1.0, 0.0, 0.0, 1.0]))
        .unwrap();

    let mut engine = TopEngine::new(ctx.clone());
    let mut cook = CookEngine::new();
    let mut time = CookContext::default();
    let mut history = History::default();

    let frame = |graph: &Graph, engine: &mut TopEngine, cook: &mut CookEngine, t: &CookContext| {
        engine.begin_frame();
        cook.cook_frame(graph, &[node], t, engine).unwrap();
        engine.end_frame();
        let tex = engine.output(graph, node).unwrap();
        let (_, _, pixels) = read_pixels_rgba8(&ctx, tex).unwrap();
        [pixels[0], pixels[1], pixels[2]]
    };

    assert_eq!(frame(&graph, &mut engine, &mut cook, &time), [255, 0, 0]);

    history.checkpoint(&graph, "color");
    graph
        .set_param(node, "color", Value::Vec4([0.0, 0.0, 1.0, 1.0]))
        .unwrap();
    history.end_gesture();
    time.advance(1.0 / 60.0);
    assert_eq!(frame(&graph, &mut engine, &mut cook, &time), [0, 0, 255]);

    graph = history.undo(&graph).unwrap();
    time.advance(1.0 / 60.0);
    assert_eq!(
        frame(&graph, &mut engine, &mut cook, &time),
        [255, 0, 0],
        "the restored graph must re-cook, not serve the cache from before it"
    );

    graph = history.redo(&graph).unwrap();
    time.advance(1.0 / 60.0);
    assert_eq!(frame(&graph, &mut engine, &mut cook, &time), [0, 0, 255]);
}

#[test]
fn undoing_a_delete_brings_the_texture_back() {
    let ctx = gpu_or_skip!();
    let registry = ops::registry();

    let mut graph = Graph::new();
    let root = graph.root();
    let src = graph
        .create(root, registry.get("constantTOP").unwrap(), Some("c1"))
        .unwrap();
    let level = graph
        .create(root, registry.get("levelTOP").unwrap(), Some("level1"))
        .unwrap();
    graph.connect(src, level, 0).unwrap();
    graph.set_param(src, "resw", Value::Int(32)).unwrap();
    graph.set_param(src, "resh", Value::Int(32)).unwrap();
    graph
        .set_param(src, "color", Value::Vec4([0.0, 1.0, 0.0, 1.0]))
        .unwrap();

    let mut engine = TopEngine::new(ctx.clone());
    let mut cook = CookEngine::new();
    let mut time = CookContext::default();
    let mut history = History::default();

    engine.begin_frame();
    cook.cook_frame(&graph, &[level], &time, &mut engine)
        .unwrap();
    engine.end_frame();

    // Delete the source out from under the chain, the way the editor does,
    // including telling the engine to forget it.
    history.checkpoint(&graph, "delete");
    history.end_gesture();
    engine.forget(src);
    cook.forget(src);
    graph.remove(src).unwrap();

    // Undo, and the whole chain has to come back and render — the id is the
    // same one, so anything the engine remembered about it had better not be
    // mistaken for a current result.
    graph = history.undo(&graph).unwrap();
    assert_eq!(graph.node(level).inputs[0], Some(src), "the wire returned");

    time.advance(1.0 / 60.0);
    engine.begin_frame();
    cook.cook_frame(&graph, &[level], &time, &mut engine)
        .unwrap();
    engine.end_frame();
    let tex = engine.output(&graph, level).unwrap();
    let (_, _, pixels) = read_pixels_rgba8(&ctx, tex).unwrap();
    assert_eq!([pixels[0], pixels[1], pixels[2]], [0, 255, 0]);
}
