//! ISF import, end to end.
//!
//! The unit tests in `isf.rs` check that the header becomes parameters and
//! that the `#define`s name the right uniform slots. What they cannot check is
//! the thing most likely to be wrong: whether the slots the *shader* was told
//! about are the slots the *engine* actually fills. That agreement only shows
//! up in pixels, so this test dials a parameter and reads the framebuffer.

use otd_core::{CookContext, CookEngine, Graph, Value};
use otd_gpu::{GpuContext, TopEngine, isf, ops, read_pixels_rgba8};

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

/// Five inputs, deliberately spanning every packing case: a scalar, a colour
/// that has to start a fresh vector, and scalars after it that share one.
const SHADER: &str = r#"/*{
    "DESCRIPTION": "packing probe",
    "INPUTS": [
        { "NAME": "red",   "TYPE": "float", "DEFAULT": 0.0, "MIN": 0.0, "MAX": 1.0 },
        { "NAME": "tint",  "TYPE": "color", "DEFAULT": [0.0, 1.0, 0.0, 1.0] },
        { "NAME": "green", "TYPE": "float", "DEFAULT": 0.0, "MIN": 0.0, "MAX": 1.0 },
        { "NAME": "blue",  "TYPE": "float", "DEFAULT": 0.0, "MIN": 0.0, "MAX": 1.0 },
        { "NAME": "on",    "TYPE": "bool",  "DEFAULT": 1 }
    ]
}*/

void main() {
    vec3 rgb = vec3(red, green, blue) * tint.rgb;
    gl_FragColor = vec4(on ? rgb : vec3(0.0), 1.0);
}
"#;

fn px(pixels: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

#[test]
fn an_imported_shader_reads_the_values_the_engine_writes() {
    let ctx = gpu_or_skip!();
    let registry = ops::registry();

    let mut graph = Graph::new();
    let root = graph.root();
    let node = graph
        .create(root, registry.get(ops::GLSL).unwrap(), Some("effect1"))
        .unwrap();

    let imported = isf::import(SHADER).expect("the header parses");
    isf::apply(&mut graph, node, &imported);

    // The ISF inputs are now ordinary parameters, addressed by their ISF name.
    graph.set_param(node, "resw", Value::Int(64)).unwrap();
    graph.set_param(node, "resh", Value::Int(32)).unwrap();
    graph.set_param(node, "red", Value::Float(1.0)).unwrap();
    graph.set_param(node, "green", Value::Float(1.0)).unwrap();
    graph.set_param(node, "blue", Value::Float(0.0)).unwrap();
    graph
        .set_param(node, "tint", Value::Vec4([1.0, 0.5, 1.0, 1.0]))
        .unwrap();

    let mut engine = TopEngine::new(ctx.clone());
    let mut cook = CookEngine::new();
    let mut time = CookContext::default();

    let run = |graph: &Graph, engine: &mut TopEngine, cook: &mut CookEngine, time: &CookContext| {
        engine.begin_frame();
        cook.cook_frame(graph, &[node], time, engine).unwrap();
        engine.end_frame();
        assert!(
            engine.shader_error(node).is_none(),
            "the imported GLSL must compile: {:?}",
            engine.shader_error(node)
        );
        let tex = engine.output(graph, node).unwrap();
        let (w, _, pixels) = read_pixels_rgba8(&ctx, tex).unwrap();
        px(&pixels, w, 10, 10)
    };

    // red=1 * tint.r=1 -> full; green=1 * tint.g=0.5 -> half; blue=0 -> none.
    // Anything else means a `#define` is pointing at a slot the engine filled
    // with a different parameter.
    let p = run(&graph, &mut engine, &mut cook, &time);
    assert_eq!(p[0], 255, "red: {p:?}");
    assert!(
        (p[1] as i32 - 128).abs() <= 2,
        "green should be halved: {p:?}"
    );
    assert_eq!(p[2], 0, "blue: {p:?}");

    // And the bool, packed as a component of the same vector as the scalars.
    graph.set_param(node, "on", Value::Bool(false)).unwrap();
    time.advance(1.0 / 60.0);
    let p = run(&graph, &mut engine, &mut cook, &time);
    assert_eq!(
        [p[0], p[1], p[2]],
        [0, 0, 0],
        "the bool gates the output: {p:?}"
    );
}

/// The commonest ISF shape by far: a filter that reads its input image.
const FILTER: &str = r#"/*{
    "INPUTS": [
        { "NAME": "inputImage", "TYPE": "image" },
        { "NAME": "amount", "TYPE": "float", "DEFAULT": 1.0, "MIN": 0.0, "MAX": 1.0 }
    ]
}*/

void main() {
    vec4 src = IMG_THIS_PIXEL(inputImage);
    gl_FragColor = vec4(mix(src.rgb, 1.0 - src.rgb, amount), src.a);
}
"#;

#[test]
fn an_imported_filter_reads_the_top_wired_to_it() {
    let ctx = gpu_or_skip!();
    let registry = ops::registry();

    let mut graph = Graph::new();
    let root = graph.root();
    let src = graph
        .create(root, registry.get("constantTOP").unwrap(), Some("src1"))
        .unwrap();
    let node = graph
        .create(root, registry.get(ops::GLSL).unwrap(), Some("effect1"))
        .unwrap();
    graph.connect(src, node, 0).unwrap();

    graph.set_param(src, "resw", Value::Int(64)).unwrap();
    graph.set_param(src, "resh", Value::Int(32)).unwrap();
    graph
        .set_param(src, "color", Value::Vec4([1.0, 0.0, 0.0, 1.0]))
        .unwrap();

    isf::apply(&mut graph, node, &isf::import(FILTER).unwrap());

    let mut engine = TopEngine::new(ctx.clone());
    let mut cook = CookEngine::new();
    let time = CookContext::default();
    engine.begin_frame();
    cook.cook_frame(&graph, &[node], &time, &mut engine)
        .unwrap();
    engine.end_frame();
    assert!(
        engine.shader_error(node).is_none(),
        "{:?}",
        engine.shader_error(node)
    );

    // `IMG_THIS_PIXEL(inputImage)` has to reach the constant TOP wired to
    // input 0 — an image input is a wire, and this is the wire.
    let tex = engine.output(&graph, node).unwrap();
    let (w, _, pixels) = read_pixels_rgba8(&ctx, tex).unwrap();
    let p = px(&pixels, w, 10, 10);
    assert_eq!(
        [p[0], p[1], p[2]],
        [0, 255, 255],
        "red inverted is cyan: {p:?}"
    );
}

#[test]
fn an_imported_shader_survives_a_save_and_load() {
    let registry = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let node = graph
        .create(root, registry.get(ops::GLSL).unwrap(), Some("effect1"))
        .unwrap();
    let imported = isf::import(SHADER).unwrap();
    isf::apply(&mut graph, node, &imported);
    graph.set_param(node, "red", Value::Float(0.75)).unwrap();

    // An imported effect is an ordinary node, so it has to round-trip like
    // one — the shader body and the custom parameters both.
    let text = otd_core::Project::from_graph(&graph, &registry, 60.0)
        .to_ron()
        .unwrap();
    let loaded = otd_core::Project::from_ron(&text)
        .unwrap()
        .to_graph(&registry)
        .unwrap();
    let id = loaded.find("/effect1").unwrap();
    let n = loaded.node(id);
    assert_eq!(n.param("red").unwrap().value, Value::Float(0.75));
    assert_eq!(
        n.param("tint").unwrap().value,
        Value::Vec4([0.0, 1.0, 0.0, 1.0])
    );
    assert!(
        n.param("source")
            .unwrap()
            .value
            .as_str()
            .contains("#define red U.p0.x"),
        "the shader body travels with the node"
    );
}

#[test]
fn re_importing_replaces_the_old_parameters_rather_than_piling_up() {
    let registry = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let node = graph
        .create(root, registry.get(ops::GLSL).unwrap(), Some("effect1"))
        .unwrap();
    isf::apply(&mut graph, node, &isf::import(SHADER).unwrap());
    assert!(graph.node(node).param("tint").is_some());

    let other = r#"/*{ "INPUTS": [ { "NAME": "amount", "TYPE": "float" } ] }*/
void main() { gl_FragColor = vec4(amount); }"#;
    isf::apply(&mut graph, node, &isf::import(other).unwrap());

    assert!(graph.node(node).param("amount").is_some());
    assert!(
        graph.node(node).param("tint").is_none(),
        "a dial from the previous shader would sit there doing nothing"
    );
    // The operator's own parameters are untouched.
    assert!(graph.node(node).param("resw").is_some());
}
