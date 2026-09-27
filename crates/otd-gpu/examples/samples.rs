//! Render one frame of every TouchDesigner-parity TOP, for looking at.
use otd_core::{CookContext, CookEngine, Graph, NodeId, OpRegistry, Value};
use otd_gpu::{GpuContext, TopEngine, ops, read_pixels_rgba8};
use std::path::Path;

const W: i64 = 320;
const H: i64 = 180;

fn shot(ctx: &GpuContext, graph: &Graph, node: NodeId, name: &str) {
    let mut engine = TopEngine::new(ctx.clone());
    let mut cook = CookEngine::new();
    engine.begin_frame();
    cook.pull(graph, node, &CookContext::default(), &mut engine)
        .unwrap();
    engine.end_frame();
    let tex = engine.output(graph, node).unwrap().clone();
    let (w, h, px) = read_pixels_rgba8(ctx, &tex).unwrap();
    let img = image::RgbaImage::from_raw(w, h, px).unwrap();
    let dir = Path::new("/tmp/otd-samples");
    std::fs::create_dir_all(dir).unwrap();
    img.save(dir.join(format!("{name}.png"))).unwrap();
}

fn mk(
    g: &mut Graph,
    r: &OpRegistry,
    root: NodeId,
    op: &str,
    ins: &[NodeId],
    ps: &[(&str, Value)],
) -> NodeId {
    let n = g.create(root, r.get(op).unwrap(), None).unwrap();
    for (i, s) in ins.iter().enumerate() {
        g.connect(*s, n, i).unwrap();
    }
    for (k, v) in ps {
        g.set_param(n, k, v.clone()).unwrap();
    }
    n
}

fn res(g: &mut Graph, n: NodeId) {
    g.set_param(n, "resw", Value::Int(W)).unwrap();
    g.set_param(n, "resh", Value::Int(H)).unwrap();
}

fn main() {
    let ctx = GpuContext::headless().expect("gpu");
    let r = ops::registry();
    let mut g = Graph::new();
    let root = g.root();

    // ---- the source: coloured cloud, a warm-to-cool wash, a hard disc.
    let noise = mk(
        &mut g,
        &r,
        root,
        "noiseTOP",
        &[],
        &[
            ("period", Value::Float(0.30)),
            ("harmonics", Value::Int(5)),
            ("monochrome", Value::Bool(false)),
            ("exponent", Value::Float(1.4)),
        ],
    );
    res(&mut g, noise);
    let wash = mk(
        &mut g,
        &r,
        root,
        "rampTOP",
        &[],
        &[
            ("type", Value::Str("radial".into())),
            ("color1", Value::Vec4([1.0, 0.72, 0.20, 1.0])),
            ("color2", Value::Vec4([0.06, 0.10, 0.42, 1.0])),
        ],
    );
    res(&mut g, wash);
    let tinted = mk(
        &mut g,
        &r,
        root,
        "compositeTOP",
        &[wash, noise],
        &[
            ("operation", Value::Str("screen".into())),
            ("opacity", Value::Float(0.75)),
        ],
    );
    // A crisp disc and some type: a hard curved edge and fine detail, so the
    // edge operators have something to bite on and the blends have real alpha.
    let disc = mk(
        &mut g,
        &r,
        root,
        "circleTOP",
        &[],
        &[
            ("radius", Value::Vec2([0.34, 0.34])),
            ("fill", Value::Vec4([0.98, 0.98, 1.0, 0.92])),
        ],
    );
    res(&mut g, disc);
    let label = mk(
        &mut g,
        &r,
        root,
        "textTOP",
        &[],
        &[
            ("text", Value::Str("OTD".into())),
            ("size", Value::Int(64)),
            ("color", Value::Vec4([0.05, 0.06, 0.20, 1.0])),
        ],
    );
    res(&mut g, label);
    let plate = mk(&mut g, &r, root, "compositeTOP", &[tinted, disc], &[]);
    let src = mk(&mut g, &r, root, "compositeTOP", &[plate, label], &[]);
    shot(&ctx, &g, src, "00-source");

    // ---- helper inputs
    let radial = mk(
        &mut g,
        &r,
        root,
        "rampTOP",
        &[],
        &[("type", Value::Str("radial".into()))],
    );
    res(&mut g, radial);
    let ramp_h = mk(&mut g, &r, root, "rampTOP", &[], &[]);
    res(&mut g, ramp_h);
    let ramp_v = mk(
        &mut g,
        &r,
        root,
        "rampTOP",
        &[],
        &[("type", Value::Str("vertical".into()))],
    );
    res(&mut g, ramp_v);
    // An identity UV map, then the same map tiled 2x2.
    let uv = mk(
        &mut g,
        &r,
        root,
        "reorderTOP",
        &[ramp_h, ramp_v],
        &[
            ("red", Value::Str("input1 r".into())),
            ("green", Value::Str("input2 r".into())),
            ("blue", Value::Str("zero".into())),
            ("alpha", Value::Str("one".into())),
        ],
    );
    let uv2 = mk(
        &mut g,
        &r,
        root,
        "tileTOP",
        &[uv],
        &[("repeat", Value::Vec2([2.0, 2.0]))],
    );

    let f = |v: f64| Value::Float(v);
    let s = |v: &str| Value::Str(v.into());

    let one_in: Vec<(&str, &str, Vec<(&str, Value)>)> = vec![
        ("monochrome", "monochromeTOP", vec![]),
        ("rgbtohsv", "rgbtohsvTOP", vec![]),
        (
            "channelmix",
            "channelmixTOP",
            vec![
                ("red", Value::Vec4([0.0, 1.0, 0.0, 0.0])),
                ("green", Value::Vec4([0.0, 0.0, 1.0, 0.0])),
                ("blue", Value::Vec4([1.0, 0.0, 0.0, 0.0])),
            ],
        ),
        (
            "rgbkey",
            "rgbkeyTOP",
            vec![
                ("color", Value::Vec4([0.06, 0.10, 0.42, 1.0])),
                ("tolerance", f(0.35)),
                ("softness", f(0.25)),
                ("output", s("matte")),
            ],
        ),
        (
            "lumalevel",
            "lumalevelTOP",
            vec![("contrast", f(2.2)), ("brightness", f(1.15))],
        ),
        ("function", "functionTOP", vec![("function", s("sqrt"))]),
        (
            "limit",
            "limitTOP",
            vec![("mode", s("quantize")), ("step", f(0.2))],
        ),
        (
            "emboss",
            "embossTOP",
            vec![("width", f(2.0)), ("strength", f(6.0))],
        ),
        ("slope", "slopeTOP", vec![("strength", f(6.0))]),
        ("normalmap", "normalmapTOP", vec![("strength", f(8.0))]),
        ("antialias", "antialiasTOP", vec![]),
        (
            "convolve",
            "convolveTOP",
            vec![
                ("row0", Value::Vec3([0.0, -1.0, 0.0])),
                ("row1", Value::Vec3([-1.0, 5.0, -1.0])),
                ("row2", Value::Vec3([0.0, -1.0, 0.0])),
                ("normalize", Value::Bool(false)),
                ("spread", f(2.0)),
            ],
        ),
        (
            "cornerpin",
            "cornerpinTOP",
            vec![
                ("topleft", Value::Vec2([0.18, 0.92])),
                ("topright", Value::Vec2([0.82, 0.92])),
                ("bottomleft", Value::Vec2([-0.05, 0.05])),
                ("bottomright", Value::Vec2([1.05, 0.05])),
            ],
        ),
        (
            "crop",
            "cropTOP",
            vec![
                ("left", f(0.30)),
                ("right", f(0.70)),
                ("bottom", f(0.20)),
                ("top", f(0.80)),
                ("resw", Value::Int(W)),
                ("resh", Value::Int(H)),
            ],
        ),
        (
            "fit",
            "fitTOP",
            vec![
                ("mode", s("fit")),
                ("resw", Value::Int(W)),
                ("resh", Value::Int(W)),
                ("background", Value::Vec4([0.10, 0.10, 0.12, 1.0])),
            ],
        ),
        (
            "lensdistort",
            "lensdistortTOP",
            vec![("k1", f(-0.45)), ("scale", f(0.75))],
        ),
        (
            "tile",
            "tileTOP",
            vec![
                ("repeat", Value::Vec2([3.0, 2.0])),
                ("mirror", Value::Bool(true)),
            ],
        ),
    ];
    for (name, op, ps) in &one_in {
        let n = mk(&mut g, &r, root, op, &[src], ps);
        shot(&ctx, &g, n, name);
    }
    // hsvtorgb reads the hsv image, so it goes after rgbtohsv rather than raw.
    let to = mk(&mut g, &r, root, "rgbtohsvTOP", &[src], &[]);
    let hue = mk(
        &mut g,
        &r,
        root,
        "levelTOP",
        &[to],
        &[("brightness", f(1.0))],
    );
    let back = mk(&mut g, &r, root, "hsvtorgbTOP", &[hue], &[]);
    shot(&ctx, &g, back, "hsvtorgb");

    // ---- two-input operators
    let reorder = mk(
        &mut g,
        &r,
        root,
        "reorderTOP",
        &[src, radial],
        &[("red", s("input2 r")), ("alpha", s("one"))],
    );
    shot(&ctx, &g, reorder, "reorder");
    let matte = mk(
        &mut g,
        &r,
        root,
        "matteTOP",
        &[src, radial],
        &[
            ("source", s("luminance")),
            ("invert", Value::Bool(true)),
            ("premultiply", Value::Bool(true)),
        ],
    );
    shot(&ctx, &g, matte, "matte");
    let remap = mk(&mut g, &r, root, "remapTOP", &[src, uv2], &[]);
    shot(&ctx, &g, remap, "remap");
    // Sharp in the middle, soft at the edges: radius comes from where the
    // control image is DARK, which is the way round a depth-of-field wants.
    let lumablur = mk(
        &mut g,
        &r,
        root,
        "lumablurTOP",
        &[src, radial],
        &[("white", f(0.0)), ("black", f(14.0))],
    );
    shot(&ctx, &g, lumablur, "lumablur");

    // ---- the named blends. The arithmetic ones want a second PICTURE, the
    // alpha ones want a second SHAPE, so they get different partners.
    let bars = mk(
        &mut g,
        &r,
        root,
        "rampTOP",
        &[],
        &[
            ("color1", Value::Vec4([0.90, 0.10, 0.35, 1.0])),
            ("color2", Value::Vec4([0.10, 0.85, 0.60, 1.0])),
        ],
    );
    res(&mut g, bars);
    for op in ["add", "subtract", "multiply", "screen", "difference"] {
        let n = mk(
            &mut g,
            &r,
            root,
            &format!("{op}TOP"),
            &[src, bars],
            &[("opacity", f(0.7))],
        );
        shot(&ctx, &g, n, op);
    }
    let shape = mk(
        &mut g,
        &r,
        root,
        "circleTOP",
        &[],
        &[
            ("centre", Value::Vec2([0.62, 0.42])),
            ("radius", Value::Vec2([0.30, 0.30])),
            ("fill", Value::Vec4([0.95, 0.25, 0.15, 0.85])),
        ],
    );
    res(&mut g, shape);
    // The alpha blends need BOTH inputs to have alpha or there is nothing to
    // see: over a fully opaque background, `inside` is just the second input.
    let masked = mk(
        &mut g,
        &r,
        root,
        "matteTOP",
        &[src, disc],
        &[("premultiply", Value::Bool(true))],
    );
    for op in ["over", "under", "inside", "outside", "cross"] {
        let n = mk(&mut g, &r, root, &format!("{op}TOP"), &[masked, shape], &[]);
        shot(&ctx, &g, n, op);
    }
    println!("wrote /tmp/otd-samples");
}
