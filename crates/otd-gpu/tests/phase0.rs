//! The Phase 0 exit criterion from PLAN.md, as an executable test:
//!
//!   "wire Noise TOP -> Level TOP -> viewer at 60fps"
//!
//! Runs headless so CI can assert it, and so the same harness serves the
//! headless CLI runtime planned for Phase 5.

use std::time::Instant;

use otd_core::{CookContext, CookEngine, Graph, Value};
use otd_gpu::{GpuContext, TopEngine, ops};

const W: i64 = 1280;
const H: i64 = 720;
const FRAMES: usize = 240;

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
fn noise_level_viewer_sustains_60fps_at_720p() {
    let ctx = gpu_or_skip!();
    let mut engine = TopEngine::new(ctx.clone());
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();

    let noise = graph
        .create(root, reg.get("noiseTOP").unwrap(), None)
        .unwrap();
    let level = graph
        .create(root, reg.get("levelTOP").unwrap(), None)
        .unwrap();
    let out = graph
        .create(root, reg.get(ops::NULL).unwrap(), None)
        .unwrap();
    graph.connect(noise, level, 0).unwrap();
    graph.connect(level, out, 0).unwrap();

    graph.set_param(noise, "resw", Value::Int(W)).unwrap();
    graph.set_param(noise, "resh", Value::Int(H)).unwrap();
    // The animation is what forces a re-cook every frame.
    graph
        .set_expression(noise, "translate", "absTime * 0.15")
        .unwrap();

    let mut cook = CookEngine::new();
    let mut time = CookContext::default();

    // One warm-up frame: shader compilation and the first allocation are not
    // part of the steady-state budget.
    engine.begin_frame();
    cook.cook_frame(&graph, &[out], &time, &mut engine).unwrap();
    engine.end_frame();
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();

    let started = Instant::now();
    for _ in 0..FRAMES {
        time.advance(1.0 / 60.0);
        engine.begin_frame();
        cook.cook_frame(&graph, &[out], &time, &mut engine).unwrap();
        engine.end_frame();
    }
    // Wait for the GPU to actually finish, or we would only be timing
    // command recording.
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let per_frame_ms = started.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;

    println!("{per_frame_ms:.3} ms/frame at {W}x{H} ({FRAMES} frames)");
    assert!(
        per_frame_ms < 16.6,
        "Phase 0 exit criterion missed: {per_frame_ms:.2} ms/frame at {W}x{H}"
    );

    // The whole chain must have cooked every frame, not been served stale.
    assert_eq!(cook.cook_count(noise), FRAMES as u64 + 1);
    assert_eq!(cook.cook_count(out), FRAMES as u64 + 1);
}

#[test]
fn an_animated_chain_actually_changes_pixels() {
    let ctx = gpu_or_skip!();
    let mut engine = TopEngine::new(ctx.clone());
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();

    let noise = graph
        .create(root, reg.get("noiseTOP").unwrap(), None)
        .unwrap();
    let level = graph
        .create(root, reg.get("levelTOP").unwrap(), None)
        .unwrap();
    graph.connect(noise, level, 0).unwrap();
    graph.set_param(noise, "resw", Value::Int(64)).unwrap();
    graph.set_param(noise, "resh", Value::Int(64)).unwrap();
    graph
        .set_expression(noise, "translate", "absTime * 0.5")
        .unwrap();

    let mut cook = CookEngine::new();
    let mut time = CookContext::default();

    let mut frames = Vec::new();
    for _ in 0..2 {
        engine.begin_frame();
        cook.cook_frame(&graph, &[level], &time, &mut engine)
            .unwrap();
        engine.end_frame();
        let tex = engine.output(&graph, level).unwrap().clone();
        frames.push(otd_gpu::read_pixels_rgba8(&ctx, &tex).unwrap().2);
        time.advance(0.5);
    }

    assert_ne!(
        frames[0], frames[1],
        "an animated expression produced identical frames"
    );
    // And the noise must not be uniform.
    assert!(
        frames[0].iter().step_by(4).any(|p| *p != frames[0][0]),
        "noise output is a flat colour"
    );
}

#[test]
fn a_static_branch_costs_nothing_after_the_first_frame() {
    let ctx = gpu_or_skip!();
    let mut engine = TopEngine::new(ctx);
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();

    let ramp = graph
        .create(root, reg.get("rampTOP").unwrap(), None)
        .unwrap();
    let blur = graph
        .create(root, reg.get("blurTOP").unwrap(), None)
        .unwrap();
    graph.connect(ramp, blur, 0).unwrap();

    let mut cook = CookEngine::new();
    let mut time = CookContext::default();
    for _ in 0..60 {
        engine.begin_frame();
        cook.cook_frame(&graph, &[blur], &time, &mut engine)
            .unwrap();
        engine.end_frame();
        time.advance(1.0 / 60.0);
    }
    assert_eq!(cook.cook_count(ramp), 1);
    assert_eq!(cook.cook_count(blur), 1);
    assert_eq!(engine.passes_this_frame, 0, "no GPU work on a cached frame");
}
