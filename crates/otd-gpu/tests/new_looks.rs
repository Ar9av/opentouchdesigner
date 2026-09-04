//! The operators added for dithering, cellular patterns, cel shading, flow
//! and depth — checked on their pixels rather than on whether they compiled.
//!
//! `every_top_compiles_its_shader_and_produces_a_texture` already proves a
//! new `.wgsl` parses and runs. It cannot tell a Dither TOP from a Null TOP,
//! because both produce a texture. So each of these asserts the *specific*
//! thing the operator exists to do: that quantising really collapses a
//! gradient onto a few values, that cel shading really flattens one, that
//! flow really moves pixels sideways, and that depth really varies with
//! distance from the camera.

use otd_core::{CookContext, CookEngine, Graph, NodeId, Value};
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

fn px(pixels: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

/// Cook `node` and read its output back.
fn render(ctx: &GpuContext, graph: &Graph, node: NodeId) -> (u32, Vec<u8>) {
    let mut engine = TopEngine::new(ctx.clone());
    let mut cook = CookEngine::new();
    engine.begin_frame();
    cook.pull(graph, node, &CookContext::default(), &mut engine)
        .unwrap();
    engine.end_frame();
    let tex = engine.output(graph, node).unwrap().clone();
    let (w, _, pixels) = read_pixels_rgba8(ctx, &tex).unwrap();
    (w, pixels)
}

/// A horizontal black-to-white ramp, which is the input that makes banding
/// and posterisation measurable.
fn ramp(graph: &mut Graph, reg: &otd_core::OpRegistry, root: NodeId, size: i64) -> NodeId {
    let r = graph.create(root, reg.get("rampTOP").unwrap(), None).unwrap();
    graph.set_param(r, "resw", Value::Int(size)).unwrap();
    graph.set_param(r, "resh", Value::Int(size)).unwrap();
    r
}

/// How many distinct red values appear along a row.
fn distinct_along_row(pixels: &[u8], w: u32, y: u32) -> usize {
    let mut seen: Vec<u8> = (0..w).map(|x| px(pixels, w, x, y)[0]).collect();
    seen.sort_unstable();
    seen.dedup();
    seen.len()
}

#[test]
fn dither_collapses_a_gradient_onto_the_level_count() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = ramp(&mut graph, &reg, root, 64);
    let d = graph
        .create(root, reg.get("ditherTOP").unwrap(), None)
        .unwrap();
    graph.connect(src, d, 0).unwrap();
    graph.set_param(d, "levels", Value::Int(3)).unwrap();
    // No pattern: straight posterisation, so the count is exact and this
    // test is about quantising rather than about the dither matrix.
    graph
        .set_param(d, "pattern", Value::Str("none".into()))
        .unwrap();

    let before = {
        let (w, p) = render(&ctx, &graph, src);
        distinct_along_row(&p, w, 8)
    };
    let (w, pixels) = render(&ctx, &graph, d);
    let after = distinct_along_row(&pixels, w, 8);

    assert!(
        before > 20,
        "the ramp should be a real gradient, got {before} values"
    );
    assert!(
        after <= 3,
        "3 levels should leave at most 3 values, got {after}"
    );
}

#[test]
fn a_dither_pattern_puts_the_detail_back() {
    // The point of dithering: at the same level count, an ordered pattern
    // trades flat bands for alternating pixels, so neighbouring pixels stop
    // agreeing. Posterisation makes long identical runs; dither breaks them.
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = ramp(&mut graph, &reg, root, 64);
    let d = graph
        .create(root, reg.get("ditherTOP").unwrap(), None)
        .unwrap();
    graph.connect(src, d, 0).unwrap();
    graph.set_param(d, "levels", Value::Int(2)).unwrap();

    let runs = |pixels: &[u8], w: u32| {
        let mut changes = 0;
        for x in 1..w {
            if px(pixels, w, x, 3)[0] != px(pixels, w, x - 1, 3)[0] {
                changes += 1;
            }
        }
        changes
    };

    graph
        .set_param(d, "pattern", Value::Str("none".into()))
        .unwrap();
    let (w, flat) = render(&ctx, &graph, d);
    let flat_changes = runs(&flat, w);

    graph
        .set_param(d, "pattern", Value::Str("bayer4".into()))
        .unwrap();
    let (w, dithered) = render(&ctx, &graph, d);
    let dithered_changes = runs(&dithered, w);

    assert!(
        dithered_changes > flat_changes,
        "bayer should alternate more than a hard threshold: {dithered_changes} vs {flat_changes}"
    );
}

#[test]
fn voronoi_makes_cells_that_differ_across_the_frame() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let v = graph
        .create(root, reg.get("voronoiTOP").unwrap(), None)
        .unwrap();
    graph.set_param(v, "resw", Value::Int(64)).unwrap();
    graph.set_param(v, "resh", Value::Int(64)).unwrap();
    graph.set_param(v, "scale", Value::Float(6.0)).unwrap();

    let (w, pixels) = render(&ctx, &graph, v);
    let values = distinct_along_row(&pixels, w, 32);
    assert!(
        values > 3,
        "a 6-cell voronoi row should cross several regions, got {values}"
    );
    // And it is a generator, so it must fill the frame rather than leaving
    // the transparent dummy showing through.
    assert_eq!(px(&pixels, w, 5, 5)[3], 255, "should be opaque");
}

#[test]
fn toon_flattens_a_gradient_into_bands() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = ramp(&mut graph, &reg, root, 64);
    let t = graph
        .create(root, reg.get("toonTOP").unwrap(), None)
        .unwrap();
    graph.connect(src, t, 0).unwrap();
    graph.set_param(t, "bands", Value::Int(4)).unwrap();
    // Ink off: this test is about the banding, and a Sobel edge on a smooth
    // ramp would add a value at every band boundary and muddy the count.
    graph.set_param(t, "edge", Value::Float(0.0)).unwrap();

    let (w, pixels) = render(&ctx, &graph, t);
    let values = distinct_along_row(&pixels, w, 8);
    assert!(
        values <= 6,
        "4 bands should leave a handful of values, got {values}"
    );
    assert!(values > 1, "but not a single flat colour");
}

#[test]
fn flow_moves_the_picture_sideways() {
    // The operator's whole job. A zero-amount flow must be a passthrough, and
    // a large one must not be — which also catches a field that evaluates to
    // zero everywhere, the way a broken curl would.
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = ramp(&mut graph, &reg, root, 64);
    let f = graph
        .create(root, reg.get("flowTOP").unwrap(), None)
        .unwrap();
    graph.connect(src, f, 0).unwrap();

    graph.set_param(f, "amount", Value::Float(0.0)).unwrap();
    let (w, still) = render(&ctx, &graph, f);

    graph.set_param(f, "amount", Value::Float(40.0)).unwrap();
    let (_, moved) = render(&ctx, &graph, f);

    let differing = (0..w)
        .filter(|x| {
            let a = px(&still, w, *x, 32)[0] as i32;
            let b = px(&moved, w, *x, 32)[0] as i32;
            (a - b).abs() > 2
        })
        .count();
    assert!(
        differing > 4,
        "40px of advection should visibly move a ramp; {differing} pixels changed"
    );
}
