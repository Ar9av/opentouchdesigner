//! Built-in patches.
//!
//! These exist as code rather than as checked-in project files so the tests,
//! the headless renderer and the editor's File menu all demonstrate the same
//! thing, and so a change to an operator's parameters cannot leave a stale
//! demo behind.

use otd_core::{Graph, NodeId, OpRegistry, Value};

use crate::ops;

/// The Phase 0 patch: `noise → level → null`, animated.
pub fn starter(registry: &OpRegistry) -> (Graph, NodeId) {
    let mut graph = Graph::new();
    let root = graph.root();
    let noise = graph
        .create(root, registry.get("noiseTOP").unwrap(), None)
        .unwrap();
    let level = graph
        .create(root, registry.get("levelTOP").unwrap(), None)
        .unwrap();
    let out = graph
        .create(root, registry.get(ops::NULL).unwrap(), None)
        .unwrap();
    graph.connect(noise, level, 0).unwrap();
    graph.connect(level, out, 0).unwrap();

    graph.node_mut_quiet(noise).pos = [-360.0, -60.0];
    graph.node_mut_quiet(level).pos = [-100.0, -60.0];
    graph.node_mut_quiet(out).pos = [160.0, -60.0];

    graph.set_param(noise, "resw", Value::Int(1280)).unwrap();
    graph.set_param(noise, "resh", Value::Int(720)).unwrap();
    graph
        .set_expression(noise, "translate", "absTime * 0.15")
        .unwrap();
    graph
        .set_expression(level, "contrast", "1.5 + sin(absTime) * 0.5")
        .unwrap();
    (graph, out)
}

/// A Shadertoy-style GLSL source, used by the feedback patch. Deliberately
/// written in GLSL rather than WGSL: the point of the exercise is that a
/// shader pasted from Shadertoy runs.
pub const FEEDBACK_GLSL: &str = r#"void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = (fragCoord - 0.5 * iResolution.xy) / iResolution.y;
    float t = iTime;
    vec3 col = vec3(0.0);
    for (int i = 0; i < 3; i++) {
        float fi = float(i);
        vec2 p = uv * (1.0 + fi * 0.35);
        p += 0.34 * vec2(cos(t * 0.7 + fi * 1.7), sin(t * 0.9 + fi * 2.3));
        float d = length(p);
        float ring = smoothstep(0.14, 0.0, abs(d - 0.16 - 0.04 * sin(t * 1.3 + fi)));
        col += ring * vec3(0.55 + 0.45 * sin(fi * 2.1 + t), 0.35 + 0.2 * fi, 1.0 - 0.25 * fi);
    }
    fragColor = vec4(col, 1.0);
}
"#;

/// The Phase 1 exit criterion: a Shadertoy shader feeding a feedback loop.
///
/// ```text
///   glsl1 ────────────────────────► comp1 (maximum) ──► out1
///                                     ▲                  │
///   fb1(target /out1) ─► decay1 ─► xform1                 │
///        ▲──────────────────────────────────────────────── (last frame)
/// ```
///
/// The loop is a genuine cycle in the picture but not in the cook graph:
/// `fb1` names `out1` by parameter and reads the texture it produced last
/// frame, which is what keeps the engine acyclic (PLAN.md §4).
pub fn feedback(registry: &OpRegistry, width: i64, height: i64) -> (Graph, NodeId) {
    let mut graph = Graph::new();
    let root = graph.root();

    let glsl = graph
        .create(root, registry.get(ops::GLSL).unwrap(), Some("glsl1"))
        .unwrap();
    let fb = graph
        .create(root, registry.get(ops::FEEDBACK).unwrap(), Some("fb1"))
        .unwrap();
    let decay = graph
        .create(root, registry.get("levelTOP").unwrap(), Some("decay1"))
        .unwrap();
    let xform = graph
        .create(root, registry.get("transformTOP").unwrap(), Some("xform1"))
        .unwrap();
    let comp = graph
        .create(root, registry.get("compositeTOP").unwrap(), Some("comp1"))
        .unwrap();
    let out = graph
        .create(root, registry.get(ops::NULL).unwrap(), Some("out1"))
        .unwrap();

    graph.connect(fb, decay, 0).unwrap();
    graph.connect(decay, xform, 0).unwrap();
    // Input 0 is the shader, so the composite — and therefore the whole
    // chain — takes its resolution from the generator rather than from the
    // feedback branch, which has nothing to inherit from on frame one.
    graph.connect(glsl, comp, 0).unwrap();
    graph.connect(xform, comp, 1).unwrap();
    graph.connect(comp, out, 0).unwrap();

    graph.node_mut_quiet(glsl).pos = [-420.0, -180.0];
    graph.node_mut_quiet(fb).pos = [-420.0, 20.0];
    graph.node_mut_quiet(decay).pos = [-220.0, 20.0];
    graph.node_mut_quiet(xform).pos = [-20.0, 20.0];
    graph.node_mut_quiet(comp).pos = [200.0, -80.0];
    graph.node_mut_quiet(out).pos = [420.0, -80.0];

    graph.set_param(glsl, "resw", Value::Int(width)).unwrap();
    graph.set_param(glsl, "resh", Value::Int(height)).unwrap();
    graph
        .set_param(glsl, "language", Value::Str("glsl".into()))
        .unwrap();
    graph
        .set_param(glsl, "source", Value::Str(FEEDBACK_GLSL.into()))
        .unwrap();

    graph
        .set_param(fb, "target", Value::Str("/out1".into()))
        .unwrap();

    // Each trip round the loop dims and shrinks slightly, which is what turns
    // a feedback loop into a trail instead of a white-out.
    graph
        .set_param(decay, "brightness", Value::Float(0.94))
        .unwrap();
    graph
        .set_param(xform, "scale", Value::Vec2([1.028, 1.028]))
        .unwrap();
    graph
        .set_param(xform, "rotate", Value::Float(0.45))
        .unwrap();
    graph
        .set_param(xform, "extend", Value::Str("zero".into()))
        .unwrap();

    graph
        .set_param(comp, "operation", Value::Str("maximum".into()))
        .unwrap();

    (graph, out)
}

/// Every built-in patch, by name, for the editor's File menu and the
/// headless renderer's `--patch` flag.
pub fn by_name(name: &str, registry: &OpRegistry) -> Option<(Graph, NodeId)> {
    match name {
        "starter" => Some(starter(registry)),
        "feedback" => Some(feedback(registry, 1920, 1080)),
        _ => None,
    }
}

pub const NAMES: &[&str] = &["starter", "feedback"];
