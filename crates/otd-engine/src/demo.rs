//! Built-in patches spanning both families.
//!
//! The TOP-only patches live in `otd_gpu::demo`; this adds the ones that need
//! CHOPs and re-exports the lot under one name lookup.

use otd_core::{Graph, NodeId, OpRegistry, Value};

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

pub fn by_name(name: &str, registry: &OpRegistry) -> Option<(Graph, NodeId)> {
    match name {
        "starter" => Some(starter(registry)),
        "feedback" => Some(feedback(registry, 1920, 1080)),
        "audioreactive" => Some(audio_reactive(registry)),
        "lfo" => Some(lfo_driven(registry)),
        _ => None,
    }
}

pub const NAMES: &[&str] = &["starter", "feedback", "audioreactive", "lfo"];
