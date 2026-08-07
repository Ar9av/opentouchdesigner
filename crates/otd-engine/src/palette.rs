//! The palette: prebuilt components, ready to drop into a network.
//!
//! PLAN.md §5 Phase 6 lists "a palette of prebuilt components" in the
//! community flywheel, and the point of each entry is pedagogical as much as
//! practical: a palette component is a worked example you can step inside.
//! Each one is an ordinary component — In and Out connectors, custom
//! parameters as its API, `parent.*` expressions inside — so opening one up
//! *is* the tutorial on how to build your own.
//!
//! They are built by code rather than shipped as files so the palette can
//! never be missing, stale, or somewhere the executable is not. *Save
//! Component…* turns any instance into a `.otdc` for sharing. Internal
//! references are relative (`out1`, not `/trails1/out1`), which is what
//! `Graph::find_from` exists for — the same patch works wherever the
//! component lands.

use otd_core::{Graph, NodeId, OpRegistry, Param, Value};

/// One palette entry: a name, what it is for, and how to build it.
pub struct Item {
    pub name: &'static str,
    pub summary: &'static str,
    build: fn(&mut Graph, &OpRegistry, NodeId) -> NodeId,
}

impl Item {
    /// Build an instance under `parent` and return it.
    pub fn build(&self, graph: &mut Graph, registry: &OpRegistry, parent: NodeId) -> NodeId {
        (self.build)(graph, registry, parent)
    }
}

pub const ITEMS: &[Item] = &[
    Item {
        name: "trails",
        summary: "Feedback trails: whatever comes in leaves a decaying, drifting wake.",
        build: trails,
    },
    Item {
        name: "bloom",
        summary: "Blurred highlights added back over the source.",
        build: bloom,
    },
    Item {
        name: "vignette",
        summary: "Darkened corners, drawing the eye to the middle.",
        build: vignette,
    },
    Item {
        name: "audiolevel",
        summary: "One smoothed channel of how loud it is, ready to export.",
        build: audio_level,
    },
];

pub fn find(name: &str) -> Option<&'static Item> {
    ITEMS.iter().find(|i| i.name == name)
}

fn add(graph: &mut Graph, reg: &OpRegistry, parent: NodeId, op: &str, name: &str) -> NodeId {
    let def = reg
        .get(op)
        .unwrap_or_else(|| panic!("palette needs `{op}`"))
        .clone();
    graph.create(parent, &def, Some(name)).unwrap()
}

fn place(graph: &mut Graph, id: NodeId, x: f32, y: f32) {
    graph.node_mut_quiet(id).pos = [x, y];
}

/// The classic feedback loop, packaged: in → composite ← (feedback → decay →
/// drift), out. The knobs are the three numbers everyone ends up tuning.
fn trails(graph: &mut Graph, reg: &OpRegistry, parent: NodeId) -> NodeId {
    let comp = add(graph, reg, parent, "containerCOMP", "trails1");
    graph.add_custom_param(
        comp,
        "decay",
        Param::float(0.94).with_label("Decay").with_range(0.0, 1.0),
    );
    graph.add_custom_param(
        comp,
        "zoom",
        Param::float(1.02).with_label("Zoom").with_range(0.9, 1.1),
    );
    graph.add_custom_param(
        comp,
        "rotate",
        Param::float(0.3)
            .with_label("Rotate (deg/frame)")
            .with_range(-10.0, 10.0),
    );

    let inp = add(graph, reg, comp, "inTOP", "in1");
    let fb = add(graph, reg, comp, "feedbackTOP", "fb1");
    let decay = add(graph, reg, comp, "levelTOP", "decay1");
    let drift = add(graph, reg, comp, "transformTOP", "drift1");
    let mix = add(graph, reg, comp, "compositeTOP", "mix1");
    let out = add(graph, reg, comp, "outTOP", "out1");

    // Input first so the chain takes its resolution from the source; the
    // feedback branch has nothing to inherit from on frame one.
    graph.connect(inp, mix, 0).unwrap();
    graph.connect(fb, decay, 0).unwrap();
    graph.connect(decay, drift, 0).unwrap();
    graph.connect(drift, mix, 1).unwrap();
    graph.connect(mix, out, 0).unwrap();

    // Relative, so the loop works in every instance of this component.
    graph
        .set_param(fb, "target", Value::Str("out1".into()))
        .unwrap();
    graph
        .set_param(mix, "operation", Value::Str("maximum".into()))
        .unwrap();
    graph
        .set_expression(decay, "brightness", "parent.decay")
        .unwrap();
    graph.set_expression(drift, "scale", "parent.zoom").unwrap();
    graph
        .set_expression(drift, "rotate", "parent.rotate")
        .unwrap();

    place(graph, inp, -420.0, -140.0);
    place(graph, fb, -420.0, 40.0);
    place(graph, decay, -240.0, 40.0);
    place(graph, drift, -60.0, 40.0);
    place(graph, mix, 140.0, -60.0);
    place(graph, out, 320.0, -60.0);
    comp
}

/// Blur the image, add the blur back. The blur *is* the glow.
fn bloom(graph: &mut Graph, reg: &OpRegistry, parent: NodeId) -> NodeId {
    let comp = add(graph, reg, parent, "containerCOMP", "bloom1");
    graph.add_custom_param(
        comp,
        "size",
        Param::float(16.0)
            .with_label("Size (px)")
            .with_range(0.0, 128.0),
    );
    graph.add_custom_param(
        comp,
        "intensity",
        Param::float(1.0)
            .with_label("Intensity")
            .with_range(0.0, 4.0),
    );

    let inp = add(graph, reg, comp, "inTOP", "in1");
    let blur = add(graph, reg, comp, "blurTOP", "blur1");
    let gain = add(graph, reg, comp, "levelTOP", "gain1");
    let mix = add(graph, reg, comp, "compositeTOP", "mix1");
    let out = add(graph, reg, comp, "outTOP", "out1");

    graph.connect(inp, blur, 0).unwrap();
    graph.connect(blur, gain, 0).unwrap();
    graph.connect(inp, mix, 0).unwrap();
    graph.connect(gain, mix, 1).unwrap();
    graph.connect(mix, out, 0).unwrap();

    graph
        .set_param(mix, "operation", Value::Str("add".into()))
        .unwrap();
    graph.set_expression(blur, "size", "parent.size").unwrap();
    graph
        .set_expression(gain, "brightness", "parent.intensity")
        .unwrap();

    place(graph, inp, -420.0, -60.0);
    place(graph, blur, -240.0, 40.0);
    place(graph, gain, -60.0, 40.0);
    place(graph, mix, 140.0, -60.0);
    place(graph, out, 320.0, -60.0);
    comp
}

const VIGNETTE_WGSL: &str = "\
// p0 = radius (where the falloff starts), p1 = strength (how dark it gets).
let d = length(in.uv - vec2f(0.5, 0.5)) * 1.4142;
let falloff = smoothstep(min(U.p0.x, 0.95), 1.0, d);
let base = sample0(in.uv);
return vec4f(base.rgb * (1.0 - U.p1.x * falloff), base.a);
";

/// A GLSL TOP with its uniforms wired to custom parameters — the smallest
/// example of turning a shader into a component with knobs.
fn vignette(graph: &mut Graph, reg: &OpRegistry, parent: NodeId) -> NodeId {
    let comp = add(graph, reg, parent, "containerCOMP", "vignette1");
    graph.add_custom_param(
        comp,
        "radius",
        Param::float(0.6).with_label("Radius").with_range(0.05, 1.0),
    );
    graph.add_custom_param(
        comp,
        "strength",
        Param::float(0.8)
            .with_label("Strength")
            .with_range(0.0, 1.0),
    );

    let inp = add(graph, reg, comp, "inTOP", "in1");
    let shade = add(graph, reg, comp, "glslTOP", "shade1");
    let out = add(graph, reg, comp, "outTOP", "out1");
    graph.connect(inp, shade, 0).unwrap();
    graph.connect(shade, out, 0).unwrap();

    graph
        .set_param(shade, "source", Value::Str(VIGNETTE_WGSL.into()))
        .unwrap();
    // A scalar expression broadcasts across the vec4 uniform; the shader
    // reads .x. Two knobs would need a Python tuple — one stays legible.
    graph
        .set_expression(shade, "uniform1", "parent.radius")
        .unwrap();
    graph
        .set_expression(shade, "uniform2", "parent.strength")
        .unwrap();

    place(graph, inp, -300.0, 0.0);
    place(graph, shade, -100.0, 0.0);
    place(graph, out, 100.0, 0.0);
    comp
}

/// Microphone to one smoothed 0..1-ish channel: the thing every
/// audio-reactive patch starts by building.
fn audio_level(graph: &mut Graph, reg: &OpRegistry, parent: NodeId) -> NodeId {
    let comp = add(graph, reg, parent, "containerCOMP", "audiolevel1");
    graph.add_custom_param(
        comp,
        "gain",
        Param::float(8.0).with_label("Gain").with_range(0.0, 64.0),
    );
    graph.add_custom_param(
        comp,
        "smooth",
        Param::float(0.15)
            .with_label("Smooth (s)")
            .with_range(0.0, 2.0),
    );

    let mic = add(graph, reg, comp, "audiodeviceinCHOP", "mic1");
    let spectrum = add(graph, reg, comp, "audiospectrumCHOP", "spectrum1");
    let lag = add(graph, reg, comp, "lagCHOP", "lag1");
    let out = add(graph, reg, comp, "outCHOP", "out1");

    graph.connect(mic, spectrum, 0).unwrap();
    graph.connect(spectrum, lag, 0).unwrap();
    graph.connect(lag, out, 0).unwrap();

    graph.set_param(spectrum, "bands", Value::Int(1)).unwrap();
    graph
        .set_expression(spectrum, "gain", "parent.gain")
        .unwrap();
    // Fast up, slow down: a beat should hit instantly and fall gracefully.
    graph
        .set_expression(lag, "lagup", "parent.smooth * 0.25")
        .unwrap();
    graph
        .set_expression(lag, "lagdown", "parent.smooth")
        .unwrap();

    place(graph, mic, -300.0, 0.0);
    place(graph, spectrum, -100.0, 0.0);
    place(graph, lag, 100.0, 0.0);
    place(graph, out, 300.0, 0.0);
    comp
}
