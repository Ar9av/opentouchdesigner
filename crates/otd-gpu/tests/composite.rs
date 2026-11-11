//! Composite "over" at partial opacity is a crossfade.
//!
//! It used to weight the top colour by the opacity twice, so white over
//! black at 0.5 came out at a quarter, and every crossfade feedback trail
//! settled at about half the brightness of what was fed into it.

use otd_core::{CookContext, CookEngine, Graph, Value};
use otd_gpu::{GpuContext, TopEngine, ops, read_pixels_rgba8};

#[test]
fn over_at_half_opacity_is_halfway() {
    let ctx = match GpuContext::headless() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skipping: no GPU available ({e})");
            return;
        }
    };
    let registry = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let mut make = |name: &str, op: &str| {
        graph
            .create(root, registry.get(op).unwrap(), Some(name))
            .unwrap()
    };
    let black = make("black", "constantTOP");
    let white = make("white", "constantTOP");
    let mix = make("mix", "compositeTOP");
    for (node, c) in [(black, 0.0), (white, 1.0)] {
        graph
            .set_param(node, "color", Value::Vec4([c, c, c, 1.0]))
            .unwrap();
        graph.set_param(node, "resw", Value::Int(16)).unwrap();
        graph.set_param(node, "resh", Value::Int(16)).unwrap();
    }
    graph.connect(black, mix, 0).unwrap();
    graph.connect(white, mix, 1).unwrap();
    graph.set_param(mix, "opacity", Value::Float(0.5)).unwrap();

    let mut engine = TopEngine::new(ctx.clone());
    let mut cook = CookEngine::new();
    engine.begin_frame();
    cook.cook_frame(&graph, &[mix], &CookContext::default(), &mut engine)
        .unwrap();
    engine.end_frame();
    let tex = engine.output(&graph, mix).unwrap();
    let (_, _, pixels) = read_pixels_rgba8(&ctx, tex).unwrap();
    let r = pixels[0] as i32;
    assert!(
        (r - 128).abs() <= 3,
        "white over black at 0.5 should be half, got {r}"
    );
}
