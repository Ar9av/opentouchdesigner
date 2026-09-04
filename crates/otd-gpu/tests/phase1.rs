//! The Phase 1 exit criterion from PLAN.md:
//!
//!   "a Shadertoy-style feedback visual patched live, full-resolution"
//!
//! Full resolution means 1920x1080 with no cap — the resolution limit is one
//! of the wedge features in PLAN.md §1, so the test asserts the real number
//! rather than a convenient one.

use std::time::Instant;

use otd_core::{CookContext, CookEngine, Project, Value};
use otd_gpu::{GpuContext, TopEngine, demo, ops, read_pixels_rgba8};

const W: i64 = 1920;
const H: i64 = 1080;
const FRAMES: usize = 180;

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
fn the_feedback_patch_sustains_60fps_at_1080p() {
    let ctx = gpu_or_skip!();
    let registry = ops::registry();
    let (graph, out) = demo::feedback(&registry, W, H);
    let mut engine = TopEngine::new(ctx.clone());
    let mut cook = CookEngine::new();
    let mut time = CookContext::default();

    // Warm-up: shader compilation and first allocation are not steady state.
    for _ in 0..2 {
        engine.begin_frame();
        cook.cook_frame(&graph, &[out], &time, &mut engine).unwrap();
        engine.end_frame();
        time.advance(1.0 / 60.0);
    }
    for id in graph.walk() {
        assert!(
            engine.shader_error(id).is_none(),
            "{}: {:?}",
            graph.path(id),
            engine.shader_error(id)
        );
    }
    let t = engine.output(&graph, out).unwrap();
    assert_eq!(
        (t.key.width, t.key.height),
        (W as u32, H as u32),
        "the chain must run at full resolution, not at the feedback branch's fallback"
    );

    let started = Instant::now();
    for _ in 0..FRAMES {
        engine.begin_frame();
        cook.cook_frame(&graph, &[out], &time, &mut engine).unwrap();
        engine.end_frame();
        time.advance(1.0 / 60.0);
    }
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let per_frame_ms = started.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;

    println!("{per_frame_ms:.3} ms/frame at {W}x{H} ({FRAMES} frames)");
    assert!(
        per_frame_ms < 16.6,
        "Phase 1 exit criterion missed: {per_frame_ms:.2} ms/frame at {W}x{H}"
    );
}

#[test]
fn the_feedback_loop_actually_accumulates() {
    let ctx = gpu_or_skip!();
    let registry = ops::registry();
    // Small, so the readback is cheap; the loop behaviour is resolution
    // independent.
    let (graph, out) = demo::feedback(&registry, 128, 128);
    let mut engine = TopEngine::new(ctx.clone());
    let mut cook = CookEngine::new();
    let mut time = CookContext::default();

    for _ in 0..120 {
        engine.begin_frame();
        cook.cook_frame(&graph, &[out], &time, &mut engine).unwrap();
        engine.end_frame();
        time.advance(1.0 / 60.0);
    }

    // Compare the loop's output against what the shader alone drew on the
    // same frame. Anything lit in the output but not in the shader is trail,
    // which is the only direct evidence that the loop is accumulating rather
    // than just passing the current frame through.
    let lit = |id| {
        let tex = engine.output(&graph, id).unwrap().clone();
        let (_, _, pixels) = read_pixels_rgba8(&ctx, &tex).unwrap();
        pixels.iter().step_by(4).filter(|p| **p > 12).count()
    };
    let shader_only = lit(graph.find("/glsl1").unwrap());
    let with_trails = lit(out);

    assert!(shader_only > 0, "the shader produced nothing");
    assert!(
        with_trails > shader_only * 3 / 2,
        "trails did not accumulate: shader lit {shader_only} pixels, output lit {with_trails}"
    );
}

#[test]
fn the_feedback_patch_survives_a_save_and_load() {
    let registry = ops::registry();
    let (graph, _) = demo::feedback(&registry, W, H);
    let text = Project::from_graph(&graph, &registry, 60.0)
        .to_ron()
        .unwrap();

    // The GLSL source is a multi-line string with braces and quotes — the
    // format has to carry it back unchanged or live-coded patches are lost.
    let reloaded = Project::from_ron(&text)
        .unwrap()
        .to_graph(&registry)
        .unwrap();
    let glsl = reloaded.find("/glsl1").expect("glsl1 survived");
    assert_eq!(
        reloaded.node(glsl).params["source"].value,
        Value::Str(demo::FEEDBACK_GLSL.to_string())
    );
    assert_eq!(
        reloaded.node(glsl).params["language"].value,
        Value::Str("glsl".into())
    );

    let fb = reloaded.find("/fb1").expect("fb1 survived");
    assert_eq!(
        reloaded.node(fb).params["target"].value,
        Value::Str("/out1".into()),
        "a parameter-named reference must survive the round trip"
    );

    let text2 = Project::from_graph(&reloaded, &registry, 60.0)
        .to_ron()
        .unwrap();
    assert_eq!(text, text2, "second round trip must be byte-identical");
}

#[test]
fn a_glsl_top_only_recooks_what_it_must() {
    let ctx = gpu_or_skip!();
    let registry = ops::registry();
    let (graph, out) = demo::feedback(&registry, 64, 64);
    let mut engine = TopEngine::new(ctx);
    let mut cook = CookEngine::new();
    let mut time = CookContext::default();

    for _ in 0..30 {
        engine.begin_frame();
        cook.cook_frame(&graph, &[out], &time, &mut engine).unwrap();
        engine.end_frame();
        time.advance(1.0 / 60.0);
    }
    let glsl = graph.find("/glsl1").unwrap();
    let fb = graph.find("/fb1").unwrap();
    // A GLSL TOP is assumed animated, and Feedback is intrinsically animated,
    // so this whole patch is legitimately hot — every node cooks every frame.
    assert_eq!(cook.cook_count(glsl), 30);
    assert_eq!(cook.cook_count(fb), 30);
    assert!(cook.is_time_dependent(out));
}
