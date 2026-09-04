//! Built-in patches spanning both families.
//!
//! The TOP-only patches live in `otd_gpu::demo`; this adds the ones that need
//! CHOPs and re-exports the lot under one name lookup.

use otd_core::{Graph, NodeId, OpRegistry, Param, Value};

pub use otd_gpu::demo::{FEEDBACK_GLSL, feedback, starter};

/// The Phase 2 exit criterion: an audio-reactive visual driven by a MIDI
/// controller.
///
/// ```text
///   audioin1 ─► spectrum1 ─► lag1 ─┬─► bass  ─► math_bass  ═► noise1.period
///                                  └─► high  ─► math_high  ═► level1.brightness
///   midiin1 ─► kick ─► trigger1 ─► math_kick ══════════════► transform1.rotate
///
///   noise1 ─► level1 ─► transform1 ─► out1        (═► is an Export)
/// ```
///
/// Every device here degrades to silence when it is absent, so the patch
/// loads and runs on a machine with no interface plugged in — it simply sits
/// still until something arrives.
pub fn audio_reactive(registry: &OpRegistry) -> (Graph, NodeId) {
    let mut graph = Graph::new();
    let root = graph.root();

    let add = |graph: &mut Graph, op: &str, name: &str, pos: [f32; 2]| {
        let def = registry.get(op).unwrap().clone();
        let id = graph.create(root, &def, Some(name)).unwrap();
        graph.node_mut_quiet(id).pos = pos;
        id
    };

    // ---- the listening side
    let audio = add(&mut graph, "audiodeviceinCHOP", "audioin1", [-760.0, 120.0]);
    let spectrum = add(
        &mut graph,
        "audiospectrumCHOP",
        "spectrum1",
        [-580.0, 120.0],
    );
    let lag = add(&mut graph, "lagCHOP", "lag1", [-400.0, 120.0]);
    let bass = add(&mut graph, "selectCHOP", "bass", [-220.0, 60.0]);
    let high = add(&mut graph, "selectCHOP", "high", [-220.0, 200.0]);
    let math_bass = add(&mut graph, "mathCHOP", "math_bass", [-40.0, 60.0]);
    let math_high = add(&mut graph, "mathCHOP", "math_high", [-40.0, 200.0]);

    let midi = add(&mut graph, "midiinCHOP", "midiin1", [-760.0, 340.0]);
    let kick = add(&mut graph, "selectCHOP", "kick", [-580.0, 340.0]);
    let trigger = add(&mut graph, "triggerCHOP", "trigger1", [-400.0, 340.0]);
    let math_kick = add(&mut graph, "mathCHOP", "math_kick", [-220.0, 340.0]);

    // ---- the picture
    let noise = add(&mut graph, "noiseTOP", "noise1", [-400.0, -180.0]);
    let level = add(&mut graph, "levelTOP", "level1", [-180.0, -180.0]);
    let xform = add(&mut graph, "transformTOP", "transform1", [40.0, -180.0]);
    let out = add(&mut graph, "nullTOP", "out1", [260.0, -180.0]);

    graph.connect(audio, spectrum, 0).unwrap();
    graph.connect(spectrum, lag, 0).unwrap();
    graph.connect(lag, bass, 0).unwrap();
    graph.connect(lag, high, 0).unwrap();
    graph.connect(bass, math_bass, 0).unwrap();
    graph.connect(high, math_high, 0).unwrap();
    graph.connect(midi, kick, 0).unwrap();
    graph.connect(kick, trigger, 0).unwrap();
    graph.connect(trigger, math_kick, 0).unwrap();
    graph.connect(noise, level, 0).unwrap();
    graph.connect(level, xform, 0).unwrap();
    graph.connect(xform, out, 0).unwrap();

    let mut set = |id, key: &str, v: Value| graph.set_param(id, key, v).unwrap();

    set(spectrum, "bands", Value::Int(4));
    set(spectrum, "gain", Value::Float(24.0));
    // Fast to rise, slow to fall — the shape that reads as "reacting to
    // music" rather than as jitter.
    set(lag, "lagup", Value::Float(0.01));
    set(lag, "lagdown", Value::Float(0.25));

    set(bass, "channels", Value::Str("band1".into()));
    set(bass, "rename", Value::Str("bass".into()));
    set(high, "channels", Value::Str("band4".into()));
    set(high, "rename", Value::Str("high".into()));

    // Map band energy into a sensible parameter range, and clamp, so a loud
    // room cannot drive the visual somewhere useless.
    set(math_bass, "gain", Value::Float(-0.35));
    set(math_bass, "offset", Value::Float(0.45));
    set(math_bass, "clamp", Value::Bool(true));
    set(math_bass, "clampmin", Value::Float(0.05));
    set(math_bass, "clampmax", Value::Float(0.5));

    set(math_high, "gain", Value::Float(3.0));
    set(math_high, "offset", Value::Float(0.8));
    set(math_high, "clamp", Value::Bool(true));
    set(math_high, "clampmin", Value::Float(0.5));
    set(math_high, "clampmax", Value::Float(4.0));

    // MIDI note 36 is the kick on almost every drum machine and pad layout.
    set(midi, "notes", Value::Str("36 38 42".into()));
    set(kick, "channels", Value::Str("n36".into()));
    set(trigger, "threshold", Value::Float(0.05));
    set(trigger, "attack", Value::Float(0.01));
    set(trigger, "release", Value::Float(0.4));
    set(math_kick, "gain", Value::Float(30.0));

    set(noise, "resw", Value::Int(1280));
    set(noise, "resh", Value::Int(720));
    set(noise, "monochrome", Value::Bool(false));
    graph
        .set_expression(noise, "translate", "absTime * 0.05")
        .unwrap();

    // ---- the exports: this is the whole point of the patch
    export(&mut graph, noise, "period", "/math_bass", "bass");
    export(&mut graph, level, "brightness", "/math_high", "high");
    export(&mut graph, xform, "rotate", "/math_kick", "n36");

    (graph, out)
}

fn export(graph: &mut Graph, id: NodeId, param: &str, op_path: &str, channel: &str) {
    if let Some(p) = graph.node_mut(id).params.get_mut(param) {
        p.set_export(op_path, channel);
    }
}

/// A patch with no devices in it, for tests and for anyone without an
/// interface: an LFO drives the same parameters an audio band would.
pub fn lfo_driven(registry: &OpRegistry) -> (Graph, NodeId) {
    let mut graph = Graph::new();
    let root = graph.root();
    let add = |graph: &mut Graph, op: &str, name: &str, pos: [f32; 2]| {
        let def = registry.get(op).unwrap().clone();
        let id = graph.create(root, &def, Some(name)).unwrap();
        graph.node_mut_quiet(id).pos = pos;
        id
    };

    let lfo = add(&mut graph, "lfoCHOP", "lfo1", [-420.0, 140.0]);
    let math = add(&mut graph, "mathCHOP", "math1", [-220.0, 140.0]);
    let noise = add(&mut graph, "noiseTOP", "noise1", [-420.0, -120.0]);
    let level = add(&mut graph, "levelTOP", "level1", [-200.0, -120.0]);
    let out = add(&mut graph, "nullTOP", "out1", [20.0, -120.0]);

    graph.connect(lfo, math, 0).unwrap();
    graph.connect(noise, level, 0).unwrap();
    graph.connect(level, out, 0).unwrap();

    graph
        .set_param(lfo, "frequency", Value::Float(0.5))
        .unwrap();
    // Map -1..1 to 0.5..2.5 brightness.
    graph.set_param(math, "gain", Value::Float(1.0)).unwrap();
    graph.set_param(math, "offset", Value::Float(1.5)).unwrap();
    graph.set_param(noise, "resw", Value::Int(320)).unwrap();
    graph.set_param(noise, "resh", Value::Int(180)).unwrap();

    export(&mut graph, level, "brightness", "/math1", "lfo");
    (graph, out)
}

/// The patch the editor opens on.
///
/// It exists because the first thing a new user sees is an argument about
/// what the tool is for, and "an animated grey noise field" is a bad
/// argument. This is nine nodes and no shader, and every one of them is a
/// lesson:
///
/// ```text
///   noise1 ──► wisps1 ──┐                    (contrast the noise into sparse
///   palette1 ───────────┴─► tint1 ──┐         bright wisps, tint through a ramp)
///                                   ├─► mix1 ──► out1
///   fb1 ──► decay1 ──► zoom1 ───────┘         (last frame, dimmed and scaled up)
/// ```
///
/// The motion is not in any operator. It is in the *loop*: `fb1` reads the
/// previous frame, `zoom1` pushes it outward, `decay1` fades it, and `mix1`
/// lays this frame's wisps on top. Every frame is the last one, slightly
/// larger and slightly darker — which is what a warp tunnel is.
pub fn tunnel(registry: &OpRegistry) -> (Graph, NodeId) {
    let mut graph = Graph::new();
    let root = graph.root();
    let add = |graph: &mut Graph, op: &str, name: &str, pos: [f32; 2]| {
        let def = registry.get(op).unwrap().clone();
        let id = graph.create(root, &def, Some(name)).unwrap();
        graph.node_mut_quiet(id).pos = pos;
        id
    };

    let noise = add(&mut graph, "noiseTOP", "noise1", [-620.0, -170.0]);
    let wisps = add(&mut graph, "levelTOP", "wisps1", [-420.0, -170.0]);
    let palette = add(&mut graph, "rampTOP", "palette1", [-620.0, -20.0]);
    let tint = add(&mut graph, "compositeTOP", "tint1", [-210.0, -110.0]);
    let fb = add(&mut graph, otd_gpu::ops::FEEDBACK, "fb1", [-420.0, 160.0]);
    let decay = add(&mut graph, "levelTOP", "decay1", [-210.0, 160.0]);
    let zoom = add(&mut graph, "transformTOP", "zoom1", [0.0, 160.0]);
    let mix = add(&mut graph, "compositeTOP", "mix1", [210.0, 20.0]);
    let out = add(&mut graph, otd_gpu::ops::NULL, "out1", [420.0, 20.0]);

    graph.connect(noise, wisps, 0).unwrap();
    graph.connect(wisps, tint, 0).unwrap();
    graph.connect(palette, tint, 1).unwrap();
    graph.connect(fb, decay, 0).unwrap();
    graph.connect(decay, zoom, 0).unwrap();
    // The source is input 0, so the chain takes its resolution from the
    // generator rather than from the loop, which has nothing on frame one.
    graph.connect(tint, mix, 0).unwrap();
    graph.connect(zoom, mix, 1).unwrap();
    graph.connect(mix, out, 0).unwrap();

    let mut set = |id, key: &str, v: Value| graph.set_param(id, key, v).unwrap();
    for id in [noise, palette] {
        set(id, "resw", Value::Int(1280));
        set(id, "resh", Value::Int(720));
    }
    // Fine, monochrome noise: the colour comes from the ramp, so the palette
    // is one parameter to change rather than three.
    set(noise, "period", Value::Float(0.22));
    set(noise, "harmonics", Value::Int(6));
    set(noise, "monochrome", Value::Bool(true));

    // A black level this high is the whole trick. It throws away the middle
    // of the noise and leaves only the brightest wisps, so the loop is fed
    // sparse highlights instead of a grey wash — which is the difference
    // between a tunnel and fog.
    set(wisps, "blacklevel", Value::Float(0.42));
    set(wisps, "contrast", Value::Float(2.4));
    set(wisps, "brightness", Value::Float(1.9));

    set(palette, "color1", Value::Vec4([0.15, 0.95, 0.90, 1.0]));
    set(palette, "color2", Value::Vec4([0.95, 0.25, 0.70, 1.0]));
    set(tint, "operation", Value::Str("multiply".into()));

    // Slightly less than 1 and slightly more than 1: the two numbers that
    // decide how long the trails are and how fast they fly.
    set(decay, "brightness", Value::Float(0.965));
    set(zoom, "scale", Value::Vec2([1.035, 1.035]));
    set(zoom, "rotate", Value::Float(0.8));
    set(mix, "operation", Value::Str("maximum".into()));
    set(fb, "target", Value::Str("/out1".into()));

    graph
        .set_expression(noise, "translate", "absTime * 0.06")
        .unwrap();
    graph
        .set_expression(palette, "phase", "absTime * 0.05")
        .unwrap();
    (graph, out)
}

/// Domain-warped noise, in one GLSL TOP.
///
/// The counterweight to `tunnel`: everything there is nodes and no shader,
/// and everything here is one shader and no nodes. Both are first-class —
/// the point of the GLSL TOP is that the ceiling of a patch is not the
/// operator list.
///
/// The source is also the worked example for the escape hatch: because it
/// declares its own `@fragment` entry point it is compiled as written rather
/// than wrapped as a fragment body, which is what makes helper functions
/// possible.
pub const NEBULA_WGSL: &str = r#"// Value noise and fbm, then iq's domain warping: fbm of fbm of fbm.
// Declaring @fragment means this is used as written, so helpers are allowed.
fn hash(p: vec2f) -> f32 {
  return fract(sin(dot(p, vec2f(127.1, 311.7))) * 43758.5453);
}

fn vnoise(p: vec2f) -> f32 {
  let i = floor(p);
  let f = fract(p);
  let u = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2f(1.0, 0.0)), u.x),
             mix(hash(i + vec2f(0.0, 1.0)), hash(i + vec2f(1.0, 1.0)), u.x), u.y);
}

fn fbm(p0: vec2f) -> f32 {
  var p = p0;
  var a = 0.5;
  var v = 0.0;
  for (var i = 0; i < 5; i = i + 1) {
    v = v + a * vnoise(p);
    p = p * 2.02;
    a = a * 0.5;
  }
  return v;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let t = U.time.x * 0.15;
  let aspect = U.res.x / U.res.y;
  let uv = (in.uv - vec2f(0.5)) * vec2f(aspect, 1.0) * 2.5;

  // Each layer warps the coordinates the next one is sampled at.
  let q = vec2f(fbm(uv + vec2f(0.0, t)), fbm(uv + vec2f(5.2, 1.3 - t)));
  let r = vec2f(fbm(uv + 4.0 * q + vec2f(1.7, 9.2) + t),
                fbm(uv + 4.0 * q + vec2f(8.3, 2.8) - t));
  let f = fbm(uv + 4.0 * r);

  // Dark base, two accents, and a hot highlight only where the warp piles
  // up — contrast is what stops a noise field looking like mud.
  let shade = clamp(f * f * 3.0, 0.0, 1.0);
  var col = mix(vec3f(0.02, 0.02, 0.09), vec3f(0.05, 0.55, 0.85), shade);
  col = mix(col, vec3f(0.85, 0.15, 0.55), clamp(length(q) * 0.9 - 0.15, 0.0, 1.0));
  col = col + vec3f(1.0, 0.75, 0.35) * pow(clamp(r.x * 1.5, 0.0, 1.0), 4.0) * 1.2;
  col = col * (0.35 + 1.1 * f);
  return vec4f(col, 1.0);
}
"#;

pub fn plasma(registry: &OpRegistry) -> (Graph, NodeId) {
    let mut graph = Graph::new();
    let root = graph.root();
    let add = |graph: &mut Graph, op: &str, name: &str, pos: [f32; 2]| {
        let def = registry.get(op).unwrap().clone();
        let id = graph.create(root, &def, Some(name)).unwrap();
        graph.node_mut_quiet(id).pos = pos;
        id
    };

    let glsl = add(&mut graph, otd_gpu::ops::GLSL, "nebula1", [-200.0, 0.0]);
    let out = add(&mut graph, otd_gpu::ops::NULL, "out1", [40.0, 0.0]);
    graph.connect(glsl, out, 0).unwrap();

    graph.set_param(glsl, "resw", Value::Int(1280)).unwrap();
    graph.set_param(glsl, "resh", Value::Int(720)).unwrap();
    graph
        .set_param(glsl, "source", Value::Str(NEBULA_WGSL.into()))
        .unwrap();
    (graph, out)
}

/// Video in, through a trail, out.
///
/// The point of the patch is that once a movie is a texture, it is a texture:
/// the feedback loop, the level and the transform after it have no idea a
/// decoder was involved, and the Movie File In sits in the chain exactly
/// where a Noise TOP would.
pub fn video(registry: &OpRegistry) -> (Graph, NodeId) {
    let mut graph = Graph::new();
    let root = graph.root();
    // The clip ships beside the examples, and the reference to it is
    // relative — the same rule external components follow, so the patch keeps
    // working when the folder moves.
    graph.set_base_dir(Some(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples"),
    ));

    let add = |graph: &mut Graph, op: &str, name: &str, pos: [f32; 2]| {
        let def = registry.get(op).unwrap().clone();
        let id = graph.create(root, &def, Some(name)).unwrap();
        graph.node_mut_quiet(id).pos = pos;
        id
    };

    let movie = add(
        &mut graph,
        otd_gpu::ops::MOVIE_IN,
        "movie1",
        [-460.0, -80.0],
    );
    let fb = add(&mut graph, otd_gpu::ops::FEEDBACK, "fb1", [-460.0, 110.0]);
    let decay = add(&mut graph, "levelTOP", "decay1", [-260.0, 110.0]);
    let drift = add(&mut graph, "transformTOP", "drift1", [-60.0, 110.0]);
    let mix = add(&mut graph, "compositeTOP", "mix1", [160.0, -20.0]);
    let out = add(&mut graph, otd_gpu::ops::NULL, "out1", [380.0, -20.0]);

    graph.connect(movie, mix, 0).unwrap();
    graph.connect(fb, decay, 0).unwrap();
    graph.connect(decay, drift, 0).unwrap();
    graph.connect(drift, mix, 1).unwrap();
    graph.connect(mix, out, 0).unwrap();

    graph
        .set_param(movie, "file", Value::Str("media/testcard.mp4".into()))
        .unwrap();
    graph
        .set_param(fb, "target", Value::Str("/out1".into()))
        .unwrap();
    graph
        .set_param(mix, "operation", Value::Str("maximum".into()))
        .unwrap();
    graph
        .set_param(decay, "brightness", Value::Float(0.9))
        .unwrap();
    graph
        .set_param(drift, "scale", Value::Vec2([1.03, 1.03]))
        .unwrap();
    (graph, out)
}

/// Build one audio-visualiser component: a self-contained band-reactive
/// visual with three knobs on its node.
///
/// This is the unit the Phase 3 exit criterion asks for — a reusable thing
/// with an API, not a pile of operators. Its parameters are read inside as
/// `parent.band`, `parent.hue` and `parent.gain`, so the same network behaves
/// differently in each instance without any of it being duplicated.
pub fn visualiser(registry: &OpRegistry, graph: &mut Graph, parent: NodeId, name: &str) -> NodeId {
    let add = |graph: &mut Graph, into: NodeId, op: &str, name: &str, pos: [f32; 2]| {
        let def = registry.get(op).unwrap().clone();
        let id = graph.create(into, &def, Some(name)).unwrap();
        graph.node_mut_quiet(id).pos = pos;
        id
    };

    let comp = add(graph, parent, "containerCOMP", name, [0.0, 0.0]);
    graph.add_custom_param(
        comp,
        "band",
        Param::int(1).with_label("Band").with_range(1.0, 4.0),
    );
    graph.add_custom_param(
        comp,
        "hue",
        Param::float(0.0).with_label("Hue").with_range(0.0, 1.0),
    );
    graph.add_custom_param(
        comp,
        "gain",
        Param::float(1.0).with_label("Gain").with_range(0.0, 4.0),
    );

    // Inside: pick one band out of the incoming spectrum, smooth it, and let
    // it drive a ring whose colour comes from the component's Hue.
    let level_in = add(graph, comp, "inCHOP", "in1", [-420.0, 40.0]);
    let pick = add(graph, comp, "selectCHOP", "pick", [-240.0, 40.0]);
    let smooth = add(graph, comp, "lagCHOP", "smooth", [-60.0, 40.0]);
    let shader = add(graph, comp, "glslTOP", "ring", [-240.0, -160.0]);
    let out = add(graph, comp, "outTOP", "out1", [40.0, -160.0]);

    graph.connect(level_in, pick, 0).unwrap();
    graph.connect(pick, smooth, 0).unwrap();
    graph.connect(shader, out, 0).unwrap();

    // The band to pick is a parameter of the component, so each instance
    // listens to a different part of the spectrum.
    graph
        .set_expression(pick, "channels", "'band' + str(parent('band'))")
        .unwrap();
    graph
        .set_param(pick, "rename", Value::Str("level".into()))
        .unwrap();
    graph
        .set_param(smooth, "lagup", Value::Float(0.02))
        .unwrap();
    graph
        .set_param(smooth, "lagdown", Value::Float(0.3))
        .unwrap();

    graph.set_param(shader, "resw", Value::Int(640)).unwrap();
    graph.set_param(shader, "resh", Value::Int(640)).unwrap();
    graph
        .set_param(shader, "source", Value::Str(RING_WGSL.into()))
        .unwrap();
    // Uniform 1 carries (level, hue, gain) into the shader.
    graph
        .set_expression(
            shader,
            "uniform1",
            "[ch(me.rsplit('/', 1)[0] + '/smooth', 'level'), parent('hue'), parent('gain'), 1.0]",
        )
        .unwrap();

    comp
}

/// The visualiser's shader: a ring that swells with its band.
pub const RING_WGSL: &str = "\
let p = (in.uv - 0.5) * 2.0;
let r = length(p);
let level = U.p0.x * U.p0.z;
let ring = smoothstep(0.5, 0.0, abs(r - 0.25 - level * 0.45));
let hue = U.p0.y;
let tint = vec3<f32>(
  0.5 + 0.5 * cos(6.2831 * (hue + 0.00)),
  0.5 + 0.5 * cos(6.2831 * (hue + 0.33)),
  0.5 + 0.5 * cos(6.2831 * (hue + 0.66)),
);
return vec4<f32>(tint * ring * (0.2 + level), 1.0);
";

/// The Phase 3 exit criterion: one component, used twice, listening to
/// different bands and tinted differently — and a Table DAT of cues alongside
/// it, so the project file carries structure as well as wiring.
pub fn two_visualisers(registry: &OpRegistry) -> (Graph, NodeId) {
    let mut graph = Graph::new();
    let root = graph.root();
    let add = |graph: &mut Graph, op: &str, name: &str, pos: [f32; 2]| {
        let def = registry.get(op).unwrap().clone();
        let id = graph.create(root, &def, Some(name)).unwrap();
        graph.node_mut_quiet(id).pos = pos;
        id
    };

    let audio = add(&mut graph, "audiodeviceinCHOP", "audioin1", [-760.0, 200.0]);
    let spectrum = add(
        &mut graph,
        "audiospectrumCHOP",
        "spectrum1",
        [-560.0, 200.0],
    );
    graph.connect(audio, spectrum, 0).unwrap();
    graph.set_param(spectrum, "bands", Value::Int(4)).unwrap();
    graph
        .set_param(spectrum, "gain", Value::Float(24.0))
        .unwrap();

    let bass = visualiser(registry, &mut graph, root, "bass_vis");
    let treble = visualiser(registry, &mut graph, root, "treble_vis");
    graph.node_mut_quiet(bass).pos = [-320.0, -120.0];
    graph.node_mut_quiet(treble).pos = [-320.0, 120.0];

    graph.connect(spectrum, bass, 0).unwrap();
    graph.connect(spectrum, treble, 0).unwrap();

    // The same component, told to be different things.
    graph.set_param(bass, "band", Value::Int(1)).unwrap();
    graph.set_param(bass, "hue", Value::Float(0.02)).unwrap();
    graph.set_param(bass, "gain", Value::Float(1.4)).unwrap();
    graph.set_param(treble, "band", Value::Int(4)).unwrap();
    graph.set_param(treble, "hue", Value::Float(0.55)).unwrap();
    graph.set_param(treble, "gain", Value::Float(2.2)).unwrap();

    let comp = add(&mut graph, "compositeTOP", "comp1", [-40.0, 0.0]);
    let out = add(&mut graph, "nullTOP", "out1", [180.0, 0.0]);
    graph.connect(bass, comp, 0).unwrap();
    graph.connect(treble, comp, 1).unwrap();
    graph
        .set_param(comp, "operation", Value::Str("maximum".into()))
        .unwrap();
    graph.connect(comp, out, 0).unwrap();

    // A cue table, to show the project file carrying structured data.
    let cues = add(&mut graph, "tableDAT", "cues", [-760.0, 400.0]);
    graph
        .set_param(
            cues,
            "text",
            Value::Str("cue\tband\thue\nintro\t1\t0.02\nbuild\t2\t0.30\ndrop\t4\t0.55".into()),
        )
        .unwrap();

    (graph, out)
}

/// The Phase 4 exit criterion: instanced geometry driven by audio, post-
/// processed through a TOP chain.
///
/// ```text
///   audioin1 ─► spectrum1 ─► lag1 ═► (instance scale)
///   pattern_x ─┬─► merge1 ─────────► geo1 instances ─► render1 ─► bloom ─► out1
///   pattern_z ─┘
/// ```
///
/// One sphere, drawn a few hundred times. The positions come from Pattern
/// CHOPs — each *sample* is an instance — and the size from an audio band, so
/// the whole grid breathes with the music. The render is then a texture like
/// any other, which is the point of the family system: the blur and level
/// after it neither know nor care that a camera was involved.
pub fn instanced_audio(registry: &OpRegistry) -> (Graph, NodeId) {
    let mut graph = Graph::new();
    let root = graph.root();
    let add = |graph: &mut Graph, op: &str, name: &str, pos: [f32; 2]| {
        let def = registry.get(op).unwrap().clone();
        let id = graph.create(root, &def, Some(name)).unwrap();
        graph.node_mut_quiet(id).pos = pos;
        id
    };

    // ---- listening
    let audio = add(&mut graph, "audiodeviceinCHOP", "audioin1", [-900.0, 320.0]);
    let spectrum = add(
        &mut graph,
        "audiospectrumCHOP",
        "spectrum1",
        [-720.0, 320.0],
    );
    let lag = add(&mut graph, "lagCHOP", "lag1", [-540.0, 320.0]);
    let bass = add(&mut graph, "selectCHOP", "bass", [-360.0, 320.0]);
    let size = add(&mut graph, "mathCHOP", "size", [-180.0, 320.0]);
    graph.connect(audio, spectrum, 0).unwrap();
    graph.connect(spectrum, lag, 0).unwrap();
    graph.connect(lag, bass, 0).unwrap();
    graph.connect(bass, size, 0).unwrap();

    graph.set_param(spectrum, "bands", Value::Int(4)).unwrap();
    graph
        .set_param(spectrum, "gain", Value::Float(30.0))
        .unwrap();
    graph.set_param(lag, "lagup", Value::Float(0.01)).unwrap();
    graph.set_param(lag, "lagdown", Value::Float(0.2)).unwrap();
    graph
        .set_param(bass, "channels", Value::Str("band1".into()))
        .unwrap();
    graph
        .set_param(bass, "rename", Value::Str("sx".into()))
        .unwrap();
    // Even in silence the grid should be visible, so the band adds to a base.
    graph.set_param(size, "gain", Value::Float(1.4)).unwrap();
    graph.set_param(size, "offset", Value::Float(0.45)).unwrap();
    graph.set_param(size, "clamp", Value::Bool(true)).unwrap();
    graph
        .set_param(size, "clampmin", Value::Float(0.2))
        .unwrap();
    graph
        .set_param(size, "clampmax", Value::Float(1.6))
        .unwrap();

    // ---- instance positions: a 16x16 grid laid out by two Pattern CHOPs.
    let px = add(&mut graph, "patternCHOP", "pattern_x", [-900.0, 520.0]);
    let pz = add(&mut graph, "patternCHOP", "pattern_z", [-900.0, 640.0]);
    let merge = add(&mut graph, "mergeCHOP", "grid_pos", [-700.0, 580.0]);
    let with_size = add(&mut graph, "mergeCHOP", "instances", [-500.0, 520.0]);
    graph.connect(px, merge, 0).unwrap();
    graph.connect(pz, merge, 1).unwrap();
    graph.connect(merge, with_size, 0).unwrap();
    graph.connect(size, with_size, 1).unwrap();

    const COUNT: i64 = 256;
    for (id, name, kind, periods) in [(px, "tx", "triangle", 16.0), (pz, "tz", "ramp", 1.0)] {
        graph.set_param(id, "length", Value::Int(COUNT)).unwrap();
        graph
            .set_param(id, "name", Value::Str(name.into()))
            .unwrap();
        graph
            .set_param(id, "type", Value::Str(kind.into()))
            .unwrap();
        graph
            .set_param(id, "periods", Value::Float(periods))
            .unwrap();
        graph.set_param(id, "amplitude", Value::Float(6.0)).unwrap();
        graph.set_param(id, "offset", Value::Float(-3.0)).unwrap();
    }

    // ---- the scene
    let sphere = add(&mut graph, "sphereSOP", "sphere1", [-900.0, 60.0]);
    let mat = add(&mut graph, "pbrMAT", "mat1", [-900.0, -60.0]);
    let geo = add(&mut graph, "geometryCOMP", "geo1", [-700.0, 0.0]);
    let cam = add(&mut graph, "cameraCOMP", "cam1", [-700.0, 140.0]);
    let light = add(&mut graph, "lightCOMP", "light1", [-700.0, 260.0]);
    let render = add(&mut graph, "renderTOP", "render1", [-480.0, 0.0]);

    graph
        .set_param(sphere, "radius", Value::Float(0.16))
        .unwrap();
    graph.set_param(sphere, "rows", Value::Int(10)).unwrap();
    graph.set_param(sphere, "columns", Value::Int(14)).unwrap();
    graph
        .set_param(mat, "basecolor", Value::Vec4([0.35, 0.75, 1.0, 1.0]))
        .unwrap();
    graph
        .set_param(mat, "roughness", Value::Float(0.25))
        .unwrap();
    graph.set_param(mat, "metallic", Value::Float(0.6)).unwrap();

    graph
        .set_param(geo, "sop", Value::Str("/sphere1".into()))
        .unwrap();
    graph
        .set_param(geo, "material", Value::Str("/mat1".into()))
        .unwrap();
    graph
        .set_param(geo, "instancing", Value::Bool(true))
        .unwrap();
    graph
        .set_param(geo, "instancechop", Value::Str("/instances".into()))
        .unwrap();
    graph.set_param(geo, "ty", Value::Str("".into())).unwrap();
    // One channel of scale drives every instance — the "all of them breathe
    // together" case.
    graph.set_param(geo, "sx", Value::Str("sx".into())).unwrap();
    graph.set_param(geo, "sy", Value::Str("sx".into())).unwrap();
    graph.set_param(geo, "sz", Value::Str("sx".into())).unwrap();

    graph
        .set_param(cam, "translate", Value::Vec3([0.0, 3.5, 7.0]))
        .unwrap();
    graph
        .set_param(cam, "lookat", Value::Str("/geo1".into()))
        .unwrap();
    graph
        .set_param(light, "translate", Value::Vec3([4.0, 6.0, 3.0]))
        .unwrap();
    graph
        .set_param(light, "intensity", Value::Float(1.3))
        .unwrap();

    graph.set_param(render, "resw", Value::Int(1280)).unwrap();
    graph.set_param(render, "resh", Value::Int(720)).unwrap();
    graph
        .set_param(render, "camera", Value::Str("/cam1".into()))
        .unwrap();
    graph
        .set_param(render, "light", Value::Str("/light1".into()))
        .unwrap();
    graph
        .set_param(render, "background", Value::Vec4([0.02, 0.02, 0.04, 1.0]))
        .unwrap();

    // Slowly orbit, so the depth in the grid reads.
    graph
        .set_expression(geo, "rotate", "[0.0, absTime * 12.0, 0.0]")
        .unwrap();

    // ---- post: the render is just a texture from here on.
    let bloom = add(&mut graph, "blurTOP", "bloom", [-260.0, 100.0]);
    let bright = add(&mut graph, "levelTOP", "bright", [-260.0, -20.0]);
    let comp = add(&mut graph, "compositeTOP", "comp1", [-40.0, 0.0]);
    let out = add(&mut graph, "nullTOP", "out1", [180.0, 0.0]);
    graph.connect(render, bright, 0).unwrap();
    graph.connect(bright, bloom, 0).unwrap();
    graph.connect(render, comp, 0).unwrap();
    graph.connect(bloom, comp, 1).unwrap();
    graph.connect(comp, out, 0).unwrap();

    graph
        .set_param(bright, "blacklevel", Value::Float(0.55))
        .unwrap();
    graph.set_param(bloom, "size", Value::Float(26.0)).unwrap();
    graph
        .set_param(comp, "operation", Value::Str("add".into()))
        .unwrap();

    (graph, out)
}

/// Keyframes driving a shape: one Animation CHOP, three curves, three
/// parameters. The Phase 6 timeline made visible.
pub fn keyframes(registry: &OpRegistry) -> (Graph, NodeId) {
    let mut graph = Graph::new();
    let root = graph.root();
    let add = |graph: &mut Graph, op: &str, name: &str, pos: [f32; 2]| {
        let def = registry.get(op).unwrap().clone();
        let id = graph.create(root, &def, Some(name)).unwrap();
        graph.node_mut_quiet(id).pos = pos;
        id
    };

    let anim = add(&mut graph, "animationCHOP", "anim1", [-420.0, 140.0]);
    let ramp = add(&mut graph, "rampTOP", "ramp1", [-420.0, -140.0]);
    let xform = add(&mut graph, "transformTOP", "xform1", [-200.0, -140.0]);
    let level = add(&mut graph, "levelTOP", "level1", [0.0, -140.0]);
    let out = add(&mut graph, "nullTOP", "out1", [200.0, -140.0]);

    graph.connect(ramp, xform, 0).unwrap();
    graph.connect(xform, level, 0).unwrap();
    graph.connect(level, out, 0).unwrap();

    // Three curves, each a different interpolation, so the difference between
    // them is visible in one glance at the curve editor.
    graph
        .set_param(
            anim,
            "keys",
            Value::Str(
                "\
# channel  time  value  interpolation
rotate     0     0      smooth
rotate     4     180    smooth
scale      0     0.4    spline
scale      2     1      spline
scale      4     0.4    spline
bright     0     0.5    linear
bright     1     2      constant
bright     3     2      linear
bright     4     0.5    linear
"
                .into(),
            ),
        )
        .unwrap();
    graph
        .set_param(anim, "play", Value::Str("loop".into()))
        .unwrap();

    // A vertical ramp rather than a radial one: a radial ramp is
    // rotation-invariant, so the `rotate` curve would drive something
    // invisible and the demo would be showing two curves while claiming three.
    graph
        .set_param(ramp, "type", Value::Str("vertical".into()))
        .unwrap();
    graph.set_param(ramp, "resw", Value::Int(640)).unwrap();
    graph.set_param(ramp, "resh", Value::Int(360)).unwrap();

    export(&mut graph, xform, "rotate", "/anim1", "rotate");
    export(&mut graph, level, "brightness", "/anim1", "bright");
    // Scale is a vec2, so it takes an expression reading the same channel
    // rather than an export, which drives a single float.
    graph
        .set_expression(xform, "scale", "ch('/anim1', 'scale')")
        .unwrap();

    (graph, out)
}

pub fn by_name(name: &str, registry: &OpRegistry) -> Option<(Graph, NodeId)> {
    match name {
        "keyframes" => Some(keyframes(registry)),
        "video" => Some(video(registry)),
        "tunnel" => Some(tunnel(registry)),
        "plasma" => Some(plasma(registry)),
        "starter" => Some(starter(registry)),
        "feedback" => Some(feedback(registry, 1920, 1080)),
        "audioreactive" => Some(audio_reactive(registry)),
        "lfo" => Some(lfo_driven(registry)),
        "components" => Some(two_visualisers(registry)),
        "instances3d" => Some(instanced_audio(registry)),
        _ => None,
    }
}

pub const NAMES: &[&str] = &[
    "tunnel",
    "plasma",
    "starter",
    "feedback",
    "audioreactive",
    "lfo",
    "components",
    "instances3d",
    "keyframes",
    "video",
];
