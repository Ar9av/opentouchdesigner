//! The Phase 4 exit criterion from PLAN.md:
//!
//!   "classic TD demo — instanced geometry driven by audio, post-processed
//!    through a TOP chain"
//!
//! The three halves of that sentence are tested separately: the geometry is
//! genuinely instanced (one draw, many copies), the audio path reaches the
//! instances, and the result is an ordinary texture that a TOP chain can
//! process.

use std::time::Instant;

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

fn lit(gpu: &GpuContext, engines: &Engines, graph: &Graph, id: NodeId) -> usize {
    let tex = engines
        .top
        .output(graph, id)
        .expect("the TOP produced a texture")
        .clone();
    let (_, _, px) = read_pixels_rgba8(gpu, &tex).unwrap();
    px.chunks(4)
        .filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 60)
        .count()
}

#[test]
fn the_grid_is_one_geometry_drawn_many_times() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let (graph, out) = demo::instanced_audio(&reg);
    let mut engines = Engines::new(gpu.clone());
    let mut cook = CookEngine::new();
    run(&graph, &mut engines, &mut cook, out, 3);

    let (draws, instances) = engines.top.render_stats();
    assert_eq!(draws, 1, "the whole grid should be one draw call");
    assert_eq!(instances, 256, "256 instances from 256 CHOP samples");

    // And the sphere itself was cooked once, not 256 times.
    let sphere = graph.find("/sphere1").unwrap();
    assert_eq!(cook.cook_count(sphere), 1);
}

#[test]
fn the_render_reaches_the_top_chain() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let (graph, out) = demo::instanced_audio(&reg);
    let mut engines = Engines::new(gpu.clone());
    let mut cook = CookEngine::new();
    run(&graph, &mut engines, &mut cook, out, 3);

    let render = graph.find("/render1").unwrap();
    let raw = lit(&gpu, &engines, &graph, render);
    let processed = lit(&gpu, &engines, &graph, out);

    assert!(raw > 500, "the render should contain geometry: {raw}");
    // The bloom adds glow around every sphere, so the composited output is
    // lit over a wider area than the raw render.
    assert!(
        processed > raw,
        "the TOP chain should have changed the image: {raw} then {processed}"
    );
}

#[test]
fn an_audio_band_scales_every_instance() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let (mut graph, out) = demo::instanced_audio(&reg);

    // Stand in for the microphone: drive the size channel from something we
    // can turn, leaving the rest of the chain exactly as shipped. A Pattern
    // CHOP is used because it names its channel verbatim, where a Constant
    // CHOP would number it.
    let root = graph.root();
    let fake = graph
        .create(root, reg.get("patternCHOP").unwrap(), Some("fake_band"))
        .unwrap();
    graph
        .set_param(fake, "name", Value::Str("sx".into()))
        .unwrap();
    graph.set_param(fake, "length", Value::Int(1)).unwrap();
    graph
        .set_param(fake, "amplitude", Value::Float(0.0))
        .unwrap();
    let size = graph.find("/size").unwrap();
    graph.disconnect(size, 0).unwrap();
    graph.connect(fake, size, 0).unwrap();

    let mut engines = Engines::new(gpu.clone());
    let mut cook = CookEngine::new();

    graph.set_param(fake, "offset", Value::Float(0.0)).unwrap();
    run(&graph, &mut engines, &mut cook, out, 2);
    let quiet = lit(&gpu, &engines, &graph, out);

    graph.set_param(fake, "offset", Value::Float(1.0)).unwrap();
    run(&graph, &mut engines, &mut cook, out, 2);
    let loud = lit(&gpu, &engines, &graph, out);

    assert!(quiet > 0, "the grid should be visible in silence too");
    assert!(
        loud > quiet + quiet / 4,
        "a louder band should grow the spheres: {quiet} then {loud}"
    );
}

#[test]
fn the_scene_sustains_60fps() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let (graph, out) = demo::instanced_audio(&reg);
    let mut engines = Engines::new(gpu.clone());
    let mut cook = CookEngine::new();
    run(&graph, &mut engines, &mut cook, out, 5);

    const FRAMES: usize = 120;
    let started = Instant::now();
    run(&graph, &mut engines, &mut cook, out, FRAMES);
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let per_frame_ms = started.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;

    println!("{per_frame_ms:.3} ms/frame for 256 instances at 1280x720 plus post");
    assert!(
        per_frame_ms < 16.6,
        "Phase 4 exit criterion missed: {per_frame_ms:.2} ms/frame"
    );
}

#[test]
fn the_3d_patch_round_trips_through_the_project_format() {
    let reg = registry();
    let (graph, _) = demo::instanced_audio(&reg);
    let text = Project::from_graph(&graph, &reg, 60.0).to_ron().unwrap();
    let back = Project::from_ron(&text).unwrap().to_graph(&reg).unwrap();

    // The scene references are ordinary parameters, so they survive like
    // everything else.
    let geo = back.find("/geo1").unwrap();
    assert_eq!(
        back.node(geo).param("sop").unwrap().value.as_str(),
        "/sphere1"
    );
    assert!(back.node(geo).param("instancing").unwrap().value.as_bool());
    let sources: Vec<&str> = back.node(geo).param_sources().collect();
    assert!(
        sources.contains(&"/sphere1") && sources.contains(&"/instances"),
        "a reloaded Geometry COMP must still declare its dependencies: {sources:?}"
    );

    let text2 = Project::from_graph(&back, &reg, 60.0).to_ron().unwrap();
    assert_eq!(text, text2, "second round trip must be byte-identical");
}

#[test]
fn a_static_3d_scene_stops_costing_anything() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let add = |graph: &mut Graph, op: &str, name: &str| {
        let def = reg.get(op).unwrap().clone();
        graph.create(root, &def, Some(name)).unwrap()
    };
    let sphere = add(&mut graph, "sphereSOP", "sphere1");
    let geo = add(&mut graph, "geometryCOMP", "geo1");
    let render = add(&mut graph, "renderTOP", "render1");
    graph
        .set_param(geo, "sop", Value::Str("/sphere1".into()))
        .unwrap();
    graph.set_param(render, "resw", Value::Int(64)).unwrap();
    graph.set_param(render, "resh", Value::Int(64)).unwrap();

    let mut engines = Engines::new(gpu);
    let mut cook = CookEngine::new();
    run(&graph, &mut engines, &mut cook, render, 30);

    // The geometry is static, so it cooks once however long the render runs.
    // The Render TOP itself is assumed animated and redraws — proving a whole
    // 3D scene static would mean tracking every operator it reaches.
    assert_eq!(cook.cook_count(sphere), 1);
    assert_eq!(cook.cook_count(geo), 1);
    assert_eq!(cook.cook_count(render), 30);
}
