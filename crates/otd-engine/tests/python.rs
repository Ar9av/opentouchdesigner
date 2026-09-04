//! Python expressions driving real operators.
//!
//! PLAN.md §3 makes Python "non-negotiable for TD migration". What matters
//! here is not that Python runs, but that it runs *in the cook* — reading the
//! network, reaching a texture, and failing safely.

use otd_core::{CookContext, CookEngine, Graph, NodeId, OpRegistry, Value};
use otd_engine::{Engines, registry};
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

fn add(graph: &mut Graph, reg: &OpRegistry, parent: NodeId, op: &str, name: &str) -> NodeId {
    let def = reg.get(op).unwrap().clone();
    graph.create(parent, &def, Some(name)).unwrap()
}

#[test]
fn a_python_expression_drives_a_texture() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();

    let src = add(&mut graph, &reg, root, "constantTOP", "src");
    let level = add(&mut graph, &reg, root, "levelTOP", "level1");
    graph.connect(src, level, 0).unwrap();
    graph.set_param(src, "resw", Value::Int(16)).unwrap();
    graph.set_param(src, "resh", Value::Int(16)).unwrap();
    graph
        .set_param(src, "color", Value::Vec4([0.25, 0.25, 0.25, 1.0]))
        .unwrap();

    // A comprehension: unmistakably Python, not the built-in language.
    graph
        .set_expression(level, "brightness", "sum(x for x in range(4))")
        .unwrap();

    let mut engines = Engines::new(gpu.clone());
    if let Some(e) = engines.python_error() {
        eprintln!("skipping: no interpreter ({e})");
        return;
    }
    let mut cook = CookEngine::new();
    let time = CookContext::default();
    engines.begin_frame();
    cook.cook_frame(&graph, &[level], &time, &mut engines)
        .unwrap();
    engines.end_frame();

    let tex = engines.top.output(&graph, level).unwrap().clone();
    let (_, _, pixels) = read_pixels_rgba8(&gpu, &tex).unwrap();
    // 0.25 grey times 6 saturates to white.
    assert!(pixels[0] > 250, "expected brightness 6, got {}", pixels[0]);
}

#[test]
fn python_can_read_a_chop_channel() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();

    let lfo = add(&mut graph, &reg, root, "constantCHOP", "knob");
    let target = add(&mut graph, &reg, root, "constantCHOP", "target");
    graph.set_param(lfo, "value0", Value::Float(0.5)).unwrap();
    // `ch()` reaches the channel; the arithmetic around it is Python's.
    graph
        .set_expression(target, "value0", "round(ch('/knob', 'chan1') * 10) / 2")
        .unwrap();

    let mut engines = Engines::new(gpu);
    if engines.python_error().is_some() {
        return;
    }
    let mut cook = CookEngine::new();
    let time = CookContext::default();
    engines.begin_frame();
    cook.cook_frame(&graph, &[target], &time, &mut engines)
        .unwrap();
    engines.end_frame();

    let data = engines.chop_data(target).unwrap();
    assert_eq!(data.value("chan1"), Some(2.5));
}

#[test]
fn python_reads_a_components_custom_parameter() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();

    let comp = add(&mut graph, &reg, root, "containerCOMP", "rig");
    graph.add_custom_param(comp, "steps", otd_core::Param::int(4));
    let inner = add(&mut graph, &reg, comp, "constantCHOP", "count");
    graph
        .set_expression(inner, "value0", "parent('steps') ** 2")
        .unwrap();

    let mut engines = Engines::new(gpu);
    if engines.python_error().is_some() {
        return;
    }
    let mut cook = CookEngine::new();
    let time = CookContext::default();
    engines.begin_frame();
    cook.cook_frame(&graph, &[inner], &time, &mut engines)
        .unwrap();
    engines.end_frame();
    assert_eq!(engines.chop_data(inner).unwrap().value("chan1"), Some(16.0));
}

#[test]
fn the_built_in_language_still_handles_simple_expressions() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let c = add(&mut graph, &reg, root, "constantCHOP", "c");
    // Parses in the fast path, so it never reaches the interpreter — which is
    // the point: the common case must not pay for the GIL.
    graph
        .set_expression(c, "value0", "sin(absTime) * 2")
        .unwrap();

    let mut engines = Engines::new(gpu);
    let mut cook = CookEngine::new();
    let time = CookContext {
        abs_time: std::f64::consts::FRAC_PI_2,
        ..Default::default()
    };
    engines.begin_frame();
    cook.cook_frame(&graph, &[c], &time, &mut engines).unwrap();
    engines.end_frame();
    let v = engines.chop_data(c).unwrap().value("chan1").unwrap();
    assert!((v - 2.0).abs() < 1e-5, "got {v}");
}

#[test]
fn a_broken_python_expression_keeps_the_constant_and_reports_why() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let c = add(&mut graph, &reg, root, "constantCHOP", "c");
    graph.set_param(c, "value0", Value::Float(0.75)).unwrap();
    graph
        .set_expression(c, "value0", "[x for x in range(3)][9]")
        .unwrap();

    let mut engines = Engines::new(gpu);
    if engines.python_error().is_some() {
        return;
    }
    let mut cook = CookEngine::new();
    let time = CookContext::default();
    engines.begin_frame();
    // The cook must succeed; a bad expression is not a failed frame.
    cook.cook_frame(&graph, &[c], &time, &mut engines).unwrap();
    engines.end_frame();
    assert_eq!(engines.chop_data(c).unwrap().value("chan1"), Some(0.75));

    // And the editor can still ask what went wrong.
    let err = engines
        .python
        .borrow_mut()
        .eval("[x for x in range(3)][9]", &time.eval_ctx(), "/c")
        .unwrap_err();
    assert!(err.contains("IndexError"), "{err}");
}

#[test]
fn a_python_expression_survives_the_project_format() {
    use otd_core::Project;
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let c = add(&mut graph, &reg, root, "constantCHOP", "c");
    let source = "max(0.0, min(1.0, absTime % 2))";
    graph.set_expression(c, "value0", source).unwrap();

    let text = Project::from_graph(&graph, &reg, 60.0).to_ron().unwrap();
    let back = Project::from_ron(&text).unwrap().to_graph(&reg).unwrap();
    let p = back.node(back.find("/c").unwrap()).param("value0").unwrap();
    assert_eq!(p.expression, source);
    assert_eq!(p.mode, otd_core::ParamMode::Expression);
    // Python expressions are assumed animated, so the node stays hot.
    assert!(p.is_time_dependent());
}
