//! The TouchDesigner-parity TOPs, checked on their pixels.
//!
//! `every_top_compiles_its_shader_and_produces_a_texture` proves a shader
//! parses and runs; it cannot tell a Slope TOP from a Null TOP, because both
//! produce a texture. Thirty-two operators arrived at once, most of them a
//! rearrangement of the same four lines of sampling, which is exactly the
//! situation where a transposed pair of parameters compiles, runs, and is
//! wrong. So each of these asserts the one thing the operator exists to do,
//! against a number worked out on paper rather than read off the screen.

use otd_core::{CookContext, CookEngine, Graph, NodeId, OpRegistry, Value};
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

/// 0..1 as the byte it is stored as, so expectations can be written as the
/// arithmetic that produced them.
fn byte(v: f64) -> i32 {
    (v * 255.0).round() as i32
}

fn near(got: u8, want: f64, what: &str) {
    let want = byte(want);
    assert!(
        (got as i32 - want).abs() <= 3,
        "{what}: got {got}, expected about {want}"
    );
}

fn constant(graph: &mut Graph, reg: &OpRegistry, root: NodeId, rgba: [f64; 4]) -> NodeId {
    let n = graph
        .create(root, reg.get("constantTOP").unwrap(), None)
        .unwrap();
    graph.set_param(n, "color", Value::Vec4(rgba)).unwrap();
    graph.set_param(n, "resw", Value::Int(64)).unwrap();
    graph.set_param(n, "resh", Value::Int(64)).unwrap();
    n
}

/// A horizontal black-to-white ramp: the input that makes a lookup shift, a
/// quantisation or a gradient measurable.
fn ramp(graph: &mut Graph, reg: &OpRegistry, root: NodeId) -> NodeId {
    let n = graph
        .create(root, reg.get("rampTOP").unwrap(), None)
        .unwrap();
    graph.set_param(n, "resw", Value::Int(64)).unwrap();
    graph.set_param(n, "resh", Value::Int(64)).unwrap();
    n
}

/// Create `op`, wire `inputs` into it in order, and hand back the node.
fn chain(graph: &mut Graph, reg: &OpRegistry, root: NodeId, op: &str, inputs: &[NodeId]) -> NodeId {
    let n = graph.create(root, reg.get(op).unwrap(), None).unwrap();
    for (i, src) in inputs.iter().enumerate() {
        graph.connect(*src, n, i).unwrap();
    }
    n
}

fn distinct_along_row(pixels: &[u8], w: u32, y: u32) -> usize {
    let mut seen: Vec<u8> = (0..w).map(|x| px(pixels, w, x, y)[0]).collect();
    seen.sort_unstable();
    seen.dedup();
    seen.len()
}

// ------------------------------------------------------------ channel maths

#[test]
fn monochrome_weights_the_channels_the_way_the_mode_says() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = constant(&mut graph, &reg, root, [1.0, 0.0, 0.0, 1.0]);

    let m = chain(&mut graph, &reg, root, "monochromeTOP", &[src]);
    let (w, p) = render(&ctx, &graph, m);
    // Rec.709: pure red is 0.2126 of the light, not a third of it. That
    // difference is the whole reason the default is not `average`.
    near(px(&p, w, 8, 8)[0], 0.2126, "luminance of pure red");

    graph
        .set_param(m, "mode", Value::Str("average".into()))
        .unwrap();
    let (w, p) = render(&ctx, &graph, m);
    near(px(&p, w, 8, 8)[0], 1.0 / 3.0, "average of pure red");
}

#[test]
fn rgb_to_hsv_and_back_is_the_colour_it_started_as() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = constant(&mut graph, &reg, root, [0.6, 0.3, 0.1, 1.0]);
    let to = chain(&mut graph, &reg, root, "rgbtohsvTOP", &[src]);
    let back = chain(&mut graph, &reg, root, "hsvtorgbTOP", &[to]);

    // Hue lands in red, and for this colour it is 0.0667 of a turn — proof
    // the conversion happened rather than the node passing its input on.
    let (w, p) = render(&ctx, &graph, to);
    near(px(&p, w, 8, 8)[0], 1.0 / 15.0, "hue in the red channel");

    let (w, p) = render(&ctx, &graph, back);
    let out = px(&p, w, 8, 8);
    near(out[0], 0.6, "red survives the round trip");
    near(out[1], 0.3, "green survives the round trip");
    near(out[2], 0.1, "blue survives the round trip");
}

#[test]
fn channel_mix_reads_the_row_it_is_told_to() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = constant(&mut graph, &reg, root, [0.25, 0.75, 0.0, 1.0]);
    let mix = chain(&mut graph, &reg, root, "channelmixTOP", &[src]);
    // Red takes green, and takes half of it.
    graph
        .set_param(mix, "red", Value::Vec4([0.0, 0.5, 0.0, 0.0]))
        .unwrap();

    let (w, p) = render(&ctx, &graph, mix);
    let out = px(&p, w, 8, 8);
    near(out[0], 0.375, "red is half the green");
    near(out[1], 0.75, "green is untouched");
}

#[test]
fn reorder_pulls_a_channel_off_the_second_input() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let a = constant(&mut graph, &reg, root, [1.0, 0.0, 0.0, 1.0]);
    let b = constant(&mut graph, &reg, root, [0.0, 0.5, 0.0, 1.0]);
    let r = chain(&mut graph, &reg, root, "reorderTOP", &[a, b]);
    graph
        .set_param(r, "red", Value::Str("input2 g".into()))
        .unwrap();
    graph
        .set_param(r, "green", Value::Str("zero".into()))
        .unwrap();

    let (w, p) = render(&ctx, &graph, r);
    let out = px(&p, w, 8, 8);
    near(out[0], 0.5, "red came from input 2's green");
    near(out[1], 0.0, "green was forced to zero");
}

// -------------------------------------------------------------- keys, mattes

#[test]
fn matte_takes_its_alpha_from_the_second_input() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = constant(&mut graph, &reg, root, [1.0, 0.0, 0.0, 1.0]);
    let mask = constant(&mut graph, &reg, root, [0.5, 0.5, 0.5, 1.0]);
    let m = chain(&mut graph, &reg, root, "matteTOP", &[src, mask]);
    graph
        .set_param(m, "source", Value::Str("luminance".into()))
        .unwrap();

    let (w, p) = render(&ctx, &graph, m);
    let out = px(&p, w, 8, 8);
    near(out[3], 0.5, "alpha is the matte's luminance");
    near(out[0], 1.0, "colour is untouched without premultiply");

    graph
        .set_param(m, "premultiply", Value::Bool(true))
        .unwrap();
    let (w, p) = render(&ctx, &graph, m);
    near(
        px(&p, w, 8, 8)[0],
        0.5,
        "premultiplied colour follows alpha",
    );
}

#[test]
fn rgb_key_removes_the_colour_it_names_and_leaves_the_rest() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let green = constant(&mut graph, &reg, root, [0.0, 1.0, 0.0, 1.0]);
    let red = constant(&mut graph, &reg, root, [1.0, 0.0, 0.0, 1.0]);

    let k = chain(&mut graph, &reg, root, "rgbkeyTOP", &[green]);
    let (w, p) = render(&ctx, &graph, k);
    near(px(&p, w, 8, 8)[3], 0.0, "the key colour is keyed out");

    let k2 = chain(&mut graph, &reg, root, "rgbkeyTOP", &[red]);
    let (w, p) = render(&ctx, &graph, k2);
    near(px(&p, w, 8, 8)[3], 1.0, "a colour far from the key is kept");
}

#[test]
fn luma_level_moves_brightness_without_moving_hue() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = constant(&mut graph, &reg, root, [0.4, 0.2, 0.1, 1.0]);
    let l = chain(&mut graph, &reg, root, "lumalevelTOP", &[src]);
    graph.set_param(l, "brightness", Value::Float(2.0)).unwrap();

    let (w, p) = render(&ctx, &graph, l);
    let out = px(&p, w, 8, 8);
    // Every channel doubles, so the ratios — and therefore the hue — hold.
    near(out[0], 0.8, "red doubled");
    near(out[1], 0.4, "green doubled");
    near(out[2], 0.2, "blue doubled");
}

// ------------------------------------------------------------- value shaping

#[test]
fn function_applies_the_function_it_is_set_to() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = constant(&mut graph, &reg, root, [0.25, 0.25, 0.25, 1.0]);
    let f = chain(&mut graph, &reg, root, "functionTOP", &[src]);

    let (w, p) = render(&ctx, &graph, f);
    near(px(&p, w, 8, 8)[0], 0.25, "`none` is a pass-through");

    graph
        .set_param(f, "function", Value::Str("invert".into()))
        .unwrap();
    let (w, p) = render(&ctx, &graph, f);
    near(px(&p, w, 8, 8)[0], 0.75, "invert is 1 - x");

    graph
        .set_param(f, "function", Value::Str("square".into()))
        .unwrap();
    graph
        .set_param(f, "premultiply", Value::Float(2.0))
        .unwrap();
    let (w, p) = render(&ctx, &graph, f);
    near(
        px(&p, w, 8, 8)[0],
        0.25,
        "the pre-scale happens before the function",
    );
}

#[test]
fn limit_quantises_a_gradient_onto_its_step() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = ramp(&mut graph, &reg, root);
    let l = chain(&mut graph, &reg, root, "limitTOP", &[src]);
    graph
        .set_param(l, "mode", Value::Str("quantize".into()))
        .unwrap();
    graph.set_param(l, "step", Value::Float(0.5)).unwrap();

    let (w, p) = render(&ctx, &graph, l);
    // 0, 0.5 and 1 — nothing else can survive a step of a half.
    assert_eq!(
        distinct_along_row(&p, w, 8),
        3,
        "a 0.5 step leaves three values"
    );

    graph
        .set_param(l, "mode", Value::Str("clamp".into()))
        .unwrap();
    graph.set_param(l, "min", Value::Float(0.25)).unwrap();
    graph.set_param(l, "max", Value::Float(0.5)).unwrap();
    let (w, p) = render(&ctx, &graph, l);
    near(px(&p, w, 0, 8)[0], 0.25, "the dark end is clamped up");
    near(px(&p, w, 63, 8)[0], 0.5, "the bright end is clamped down");
}

#[test]
fn convolve_runs_the_kernel_it_is_given() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = constant(&mut graph, &reg, root, [0.25, 0.25, 0.25, 1.0]);
    let c = chain(&mut graph, &reg, root, "convolveTOP", &[src]);

    let (w, p) = render(&ctx, &graph, c);
    near(
        px(&p, w, 8, 8)[0],
        0.25,
        "the default kernel is the identity",
    );

    // A centre tap of 2 with normalising off is a gain of two.
    graph
        .set_param(c, "row1", Value::Vec3([0.0, 2.0, 0.0]))
        .unwrap();
    graph.set_param(c, "normalize", Value::Bool(false)).unwrap();
    let (w, p) = render(&ctx, &graph, c);
    near(
        px(&p, w, 8, 8)[0],
        0.5,
        "an un-normalised centre tap of 2 doubles",
    );

    // The same kernel normalised divides by the weight again, so it does not.
    graph.set_param(c, "normalize", Value::Bool(true)).unwrap();
    let (w, p) = render(&ctx, &graph, c);
    near(px(&p, w, 8, 8)[0], 0.25, "normalising cancels the gain");
}

// ------------------------------------------------------------------ geometry

#[test]
fn corner_pin_is_the_identity_at_the_corners_and_a_warp_away_from_them() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = ramp(&mut graph, &reg, root);
    let c = chain(&mut graph, &reg, root, "cornerpinTOP", &[src]);

    let (w, p) = render(&ctx, &graph, c);
    let (rw, rp) = render(&ctx, &graph, src);
    for x in [1, 20, 40, 62] {
        assert!(
            (px(&p, w, x, 8)[0] as i32 - px(&rp, rw, x, 8)[0] as i32).abs() <= 2,
            "default corners are a pass-through at x={x}"
        );
    }

    // Pull the top edge in: the top row now shows a squeezed ramp, so a
    // sample near the left is further along it than the source was.
    graph
        .set_param(c, "topleft", Value::Vec2([0.25, 1.0]))
        .unwrap();
    graph
        .set_param(c, "topright", Value::Vec2([0.75, 1.0]))
        .unwrap();
    let (w, p) = render(&ctx, &graph, c);
    assert!(
        px(&p, w, 20, 1)[0] < px(&rp, rw, 20, 1)[0],
        "a squeezed top edge samples earlier in the ramp"
    );
    assert!(
        px(&p, w, 2, 1)[3] == 0,
        "outside the pinned quad is transparent with extend `zero`"
    );
}

#[test]
fn crop_keeps_the_region_it_is_given() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = ramp(&mut graph, &reg, root);
    let c = chain(&mut graph, &reg, root, "cropTOP", &[src]);
    graph.set_param(c, "left", Value::Float(0.5)).unwrap();
    graph.set_param(c, "resw", Value::Int(64)).unwrap();
    graph.set_param(c, "resh", Value::Int(64)).unwrap();

    let (w, p) = render(&ctx, &graph, c);
    assert_eq!(w, 64, "the output is the size the parameters ask for");
    near(
        px(&p, w, 0, 8)[0],
        0.504,
        "the left edge is the middle of the ramp",
    );
    near(
        px(&p, w, 63, 8)[0],
        0.996,
        "the right edge is still the ramp's end",
    );
}

#[test]
fn fit_letterboxes_rather_than_filling_when_it_is_told_to() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = ramp(&mut graph, &reg, root);
    let f = chain(&mut graph, &reg, root, "fitTOP", &[src]);
    // A square source into a 2:1 frame.
    graph.set_param(f, "resw", Value::Int(128)).unwrap();
    graph.set_param(f, "resh", Value::Int(64)).unwrap();

    let (w, p) = render(&ctx, &graph, f);
    assert_eq!(
        px(&p, w, 1, 32)[3],
        0,
        "`fit` leaves background at the sides"
    );
    near(
        px(&p, w, 64, 32)[0],
        0.5,
        "and the middle of the ramp in the middle",
    );

    // `fill` covers the frame instead, so nothing is left transparent.
    graph
        .set_param(f, "mode", Value::Str("fill".into()))
        .unwrap();
    let (w, p) = render(&ctx, &graph, f);
    assert_eq!(px(&p, w, 1, 32)[3], 255, "`fill` covers the frame");
    near(
        px(&p, w, 1, 32)[0],
        0.012,
        "and shows the ramp's start at its left",
    );
}

#[test]
fn tile_repeats_the_image_across_the_frame() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = ramp(&mut graph, &reg, root);
    let t = chain(&mut graph, &reg, root, "tileTOP", &[src]);
    graph
        .set_param(t, "repeat", Value::Vec2([2.0, 1.0]))
        .unwrap();

    let (w, p) = render(&ctx, &graph, t);
    for x in [1u32, 5, 12] {
        assert_eq!(
            px(&p, w, x, 8)[0],
            px(&p, w, x + 32, 8)[0],
            "the second copy matches the first at x={x}"
        );
    }
    assert!(
        px(&p, w, 31, 8)[0] > px(&p, w, 32, 8)[0],
        "and the ramp restarts at the seam"
    );
}

#[test]
fn lens_distort_does_nothing_at_zero_and_moves_pixels_otherwise() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = ramp(&mut graph, &reg, root);
    let d = chain(&mut graph, &reg, root, "lensdistortTOP", &[src]);

    let (w, p) = render(&ctx, &graph, d);
    let (rw, rp) = render(&ctx, &graph, src);
    assert_eq!(
        px(&p, w, 20, 32)[0],
        px(&rp, rw, 20, 32)[0],
        "no distortion is a pass-through"
    );

    graph.set_param(d, "k1", Value::Float(-0.5)).unwrap();
    let (w, p) = render(&ctx, &graph, d);
    // Barrel is the fisheye bulge: the middle of the source is magnified
    // across the frame, so a point left of centre now reads from nearer the
    // middle of the ramp — a brighter value than the source had there.
    assert!(
        px(&p, w, 8, 32)[0] > px(&rp, rw, 8, 32)[0],
        "barrel distortion magnifies the centre"
    );

    graph.set_param(d, "k1", Value::Float(0.5)).unwrap();
    let (w, p) = render(&ctx, &graph, d);
    assert!(
        px(&p, w, 8, 32)[0] < px(&rp, rw, 8, 32)[0],
        "and pincushion, the opposite sign, does the opposite"
    );
}

#[test]
fn remap_reads_input_one_at_the_coordinates_in_input_two() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = ramp(&mut graph, &reg, root);
    // A flat map: every pixel is told to read the same place, so the whole
    // frame should come out one colour — the ramp at u = 0.25.
    let map = constant(&mut graph, &reg, root, [0.25, 0.5, 0.0, 1.0]);
    let r = chain(&mut graph, &reg, root, "remapTOP", &[src, map]);

    let (w, p) = render(&ctx, &graph, r);
    near(
        px(&p, w, 8, 8)[0],
        0.25,
        "the map's red is the u it read at",
    );
    near(px(&p, w, 50, 40)[0], 0.25, "and it is the same everywhere");

    graph.set_param(r, "amount", Value::Float(0.0)).unwrap();
    let (w, p) = render(&ctx, &graph, r);
    let (rw, rp) = render(&ctx, &graph, src);
    assert_eq!(
        px(&p, w, 50, 8)[0],
        px(&rp, rw, 50, 8)[0],
        "amount 0 is the identity map"
    );
}

// ------------------------------------------------------------ edges and blur

#[test]
fn emboss_is_flat_grey_on_a_flat_image_and_lit_on_a_gradient() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let flat = constant(&mut graph, &reg, root, [0.5, 0.5, 0.5, 1.0]);
    let e = chain(&mut graph, &reg, root, "embossTOP", &[flat]);
    let (w, p) = render(&ctx, &graph, e);
    near(px(&p, w, 8, 8)[0], 0.5, "nothing to relieve is mid grey");

    let src = ramp(&mut graph, &reg, root);
    let e2 = chain(&mut graph, &reg, root, "embossTOP", &[src]);
    let (w, p) = render(&ctx, &graph, e2);
    assert!(
        px(&p, w, 32, 32)[0] > byte(0.5) as u8 + 2,
        "a rising gradient lights up"
    );
}

#[test]
fn slope_is_the_derivative_and_points_the_way_the_ramp_rises() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = ramp(&mut graph, &reg, root);
    let s = chain(&mut graph, &reg, root, "slopeTOP", &[src]);
    graph.set_param(s, "strength", Value::Float(16.0)).unwrap();

    let (w, p) = render(&ctx, &graph, s);
    let out = px(&p, w, 32, 32);
    // d/dx over two texels of a 64-wide 0..1 ramp is 2/64; halved and scaled
    // by 16 that is 0.25, centred on 0.5.
    near(out[0], 0.75, "x slope");
    near(out[1], 0.5, "y slope is flat");
}

#[test]
fn normal_map_is_straight_up_on_a_flat_height_field() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let flat = constant(&mut graph, &reg, root, [0.5, 0.5, 0.5, 1.0]);
    let n = chain(&mut graph, &reg, root, "normalmapTOP", &[flat]);
    let (w, p) = render(&ctx, &graph, n);
    let out = px(&p, w, 8, 8);
    near(out[0], 0.5, "flat means no x tilt");
    near(out[1], 0.5, "flat means no y tilt");
    near(out[2], 1.0, "and the normal points at the viewer");

    let src = ramp(&mut graph, &reg, root);
    let n2 = chain(&mut graph, &reg, root, "normalmapTOP", &[src]);
    let (w, p) = render(&ctx, &graph, n2);
    assert!(
        px(&p, w, 32, 32)[0] < byte(0.5) as u8 - 2,
        "a rising height field tilts the normal"
    );
}

#[test]
fn anti_alias_softens_a_hard_edge_without_touching_a_flat_field() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let flat = constant(&mut graph, &reg, root, [0.4, 0.4, 0.4, 1.0]);
    let a = chain(&mut graph, &reg, root, "antialiasTOP", &[flat]);
    let (w, p) = render(&ctx, &graph, a);
    near(px(&p, w, 8, 8)[0], 0.4, "no edges, nothing to do");

    // A circle hardened to two values: the only curve in the suite, and the
    // one shape a per-axis filter cannot fake being good at.
    let circle = graph
        .create(root, reg.get("circleTOP").unwrap(), None)
        .unwrap();
    graph.set_param(circle, "resw", Value::Int(64)).unwrap();
    graph.set_param(circle, "resh", Value::Int(64)).unwrap();
    let hard = chain(&mut graph, &reg, root, "thresholdTOP", &[circle]);
    graph
        .set_param(hard, "softness", Value::Float(0.0))
        .unwrap();
    let smooth = chain(&mut graph, &reg, root, "antialiasTOP", &[hard]);

    let (hw, hp) = render(&ctx, &graph, hard);
    let (sw, sp) = render(&ctx, &graph, smooth);
    assert!(
        distinct_along_row(&sp, sw, 20) > distinct_along_row(&hp, hw, 20),
        "the edge gained intermediate values"
    );
}

#[test]
fn luma_blur_blurs_where_the_second_input_is_bright_and_not_where_it_is_dark() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = ramp(&mut graph, &reg, root);
    let dark = constant(&mut graph, &reg, root, [0.0, 0.0, 0.0, 1.0]);
    let b = chain(&mut graph, &reg, root, "lumablurTOP", &[src, dark]);

    let (w, p) = render(&ctx, &graph, b);
    let (rw, rp) = render(&ctx, &graph, src);
    assert_eq!(
        px(&p, w, 40, 8)[0],
        px(&rp, rw, 40, 8)[0],
        "a black control image means radius `black`, which is zero"
    );

    // A ramp is its own average, so blurring it changes nothing measurable in
    // the middle — the edge is where a blur has to show up.
    let bright = constant(&mut graph, &reg, root, [1.0, 1.0, 1.0, 1.0]);
    let b2 = chain(&mut graph, &reg, root, "lumablurTOP", &[src, bright]);
    graph.set_param(b2, "white", Value::Float(24.0)).unwrap();
    let (w, p) = render(&ctx, &graph, b2);
    assert!(
        px(&p, w, 0, 8)[0] > px(&rp, rw, 0, 8)[0] + 3,
        "a white control image drags the ramp's dark edge up"
    );
}

// ------------------------------------------------------------ the blend ops

/// Every named blend operator, on the same pair of inputs, against the number
/// its formula gives.
///
/// They share one shader and differ only by the index their `pack` pins, so
/// this is the test that a Subtract TOP does not quietly screen. The unit
/// test in `ops.rs` checks the index against the menu; this checks the menu
/// against arithmetic.
#[test]
fn each_named_blend_computes_its_own_formula() {
    let ctx = gpu_or_skip!();
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let base = constant(&mut graph, &reg, root, [0.8, 0.8, 0.8, 0.25]);
    let top = constant(&mut graph, &reg, root, [0.2, 0.2, 0.2, 0.5]);

    // (operator, expected rgb, expected alpha)
    let cases: &[(&str, f64, f64)] = &[
        ("addTOP", 1.0, 0.5),
        ("subtractTOP", 0.6, 0.5),
        ("multiplyTOP", 0.16, 0.5),
        ("screenTOP", 0.84, 0.5),
        ("differenceTOP", 0.6, 0.5),
        // 0.2*0.5 + 0.8*0.5, and 0.5 + 0.25*0.5
        ("overTOP", 0.5, 0.625),
        // 0.8*0.25 + 0.2*0.75, and 0.25 + 0.5*0.75
        ("underTOP", 0.35, 0.625),
        ("insideTOP", 0.2, 0.125),
        ("outsideTOP", 0.2, 0.375),
        ("crossTOP", 0.175, 0.5),
    ];

    for (op, rgb, alpha) in cases {
        let n = chain(&mut graph, &reg, root, op, &[base, top]);
        let (w, p) = render(&ctx, &graph, n);
        let out = px(&p, w, 8, 8);
        near(out[0], *rgb, &format!("{op} colour"));
        near(out[3], *alpha, &format!("{op} alpha"));
    }
}
