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

pub fn by_name(name: &str, registry: &OpRegistry) -> Option<(Graph, NodeId)> {
    match name {
        "starter" => Some(starter(registry)),
        "feedback" => Some(feedback(registry, 1920, 1080)),
        "audioreactive" => Some(audio_reactive(registry)),
        "lfo" => Some(lfo_driven(registry)),
        "components" => Some(two_visualisers(registry)),
        _ => None,
    }
}

pub const NAMES: &[&str] = &["starter", "feedback", "audioreactive", "lfo", "components"];
