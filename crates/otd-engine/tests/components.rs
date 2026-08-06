//! Component encapsulation: In and Out operators surfacing as connectors.
//!
//! PLAN.md §2.4. The property under test throughout is that a component
//! behaves exactly like an operator from outside — it wires, it cooks, it
//! caches and it animates by the same rules — while being a whole network
//! inside.

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
    let def = reg
        .get(op)
        .unwrap_or_else(|| panic!("no operator `{op}`"))
        .clone();
    graph.create(parent, &def, Some(name)).unwrap()
}

/// A component that darkens whatever is fed to it.
fn darkener(graph: &mut Graph, reg: &OpRegistry, parent: NodeId, name: &str) -> NodeId {
    let comp = add(graph, reg, parent, "containerCOMP", name);
    let inp = add(graph, reg, comp, "inTOP", "in1");
    let level = add(graph, reg, comp, "levelTOP", "level1");
    let outp = add(graph, reg, comp, "outTOP", "out1");
    graph.connect(inp, level, 0).unwrap();
    graph.connect(level, outp, 0).unwrap();
    graph
        .set_param(level, "brightness", Value::Float(0.5))
        .unwrap();
    comp
}

#[test]
fn an_in_operator_becomes_a_connector_on_the_component() {
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let comp = add(&mut graph, &reg, root, "containerCOMP", "comp1");

    assert_eq!(
        graph.node(comp).inputs.len(),
        0,
        "no In operators, no inputs"
    );

    let _in1 = add(&mut graph, &reg, comp, "inTOP", "in1");
    assert_eq!(graph.node(comp).inputs.len(), 1);
    assert_eq!(graph.node(comp).input_labels, vec!["in1"]);

    let _in2 = add(&mut graph, &reg, comp, "inTOP", "in2");
    assert_eq!(graph.node(comp).inputs.len(), 2);
    assert_eq!(graph.node(comp).input_labels, vec!["in1", "in2"]);

    // And an Out operator gives it something to present.
    assert_eq!(graph.output_family(comp), None);
    let _out = add(&mut graph, &reg, comp, "outTOP", "out1");
    assert_eq!(graph.output_family(comp), Some(otd_core::Family::Top));
}

#[test]
fn a_component_wires_like_an_operator_and_type_checks_by_its_connectors() {
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let comp = darkener(&mut graph, &reg, root, "darken");
    let noise = add(&mut graph, &reg, root, "noiseTOP", "noise1");
    let out = add(&mut graph, &reg, root, "nullTOP", "out1");

    graph
        .connect(noise, comp, 0)
        .expect("TOP into a TOP connector");
    graph.connect(comp, out, 0).expect("component into a TOP");

    // A CHOP must not fit a texture connector, even though the component's
    // own family is COMP.
    let lfo = add(&mut graph, &reg, root, "lfoCHOP", "lfo1");
    assert!(graph.connect(lfo, comp, 0).is_err());
}

#[test]
fn a_component_actually_processes_what_is_wired_to_it() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();

    let src = add(&mut graph, &reg, root, "constantTOP", "src");
    let comp = darkener(&mut graph, &reg, root, "darken");
    let out = add(&mut graph, &reg, root, "nullTOP", "out1");
    graph.connect(src, comp, 0).unwrap();
    graph.connect(comp, out, 0).unwrap();
    graph.set_param(src, "resw", Value::Int(32)).unwrap();
    graph.set_param(src, "resh", Value::Int(32)).unwrap();
    graph
        .set_param(src, "color", Value::Vec4([1.0, 1.0, 1.0, 1.0]))
        .unwrap();

    let mut engines = Engines::new(gpu.clone());
    let mut cook = CookEngine::new();
    let time = CookContext::default();
    engines.begin_frame();
    cook.cook_frame(&graph, &[out], &time, &mut engines)
        .unwrap();
    engines.end_frame();

    let tex = engines.top.output(&graph, out).unwrap().clone();
    let (_, _, pixels) = read_pixels_rgba8(&gpu, &tex).unwrap();
    // White in, brightness 0.5 inside the component, so mid grey out.
    assert!(
        (pixels[0] as i32 - 128).abs() <= 3,
        "expected the component's Level to apply, got {}",
        pixels[0]
    );

    // Pulling the outside pulled the inside.
    let inner = graph.find("/darken/level1").unwrap();
    assert!(cook.cook_count(inner) > 0);
}

#[test]
fn the_same_component_used_twice_keeps_its_own_settings() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();

    let src = add(&mut graph, &reg, root, "constantTOP", "src");
    graph.set_param(src, "resw", Value::Int(16)).unwrap();
    graph.set_param(src, "resh", Value::Int(16)).unwrap();
    graph
        .set_param(src, "color", Value::Vec4([1.0, 1.0, 1.0, 1.0]))
        .unwrap();

    let a = darkener(&mut graph, &reg, root, "darken_a");
    let b = darkener(&mut graph, &reg, root, "darken_b");
    graph.connect(src, a, 0).unwrap();
    graph.connect(src, b, 0).unwrap();
    // Same structure, different setting inside.
    let b_level = graph.find("/darken_b/level1").unwrap();
    graph
        .set_param(b_level, "brightness", Value::Float(0.25))
        .unwrap();

    let mut engines = Engines::new(gpu.clone());
    let mut cook = CookEngine::new();
    let time = CookContext::default();
    engines.begin_frame();
    cook.cook_frame(&graph, &[a, b], &time, &mut engines)
        .unwrap();
    engines.end_frame();

    let read = |id| {
        let tex = engines.top.output(&graph, id).unwrap().clone();
        read_pixels_rgba8(&gpu, &tex).unwrap().2[0]
    };
    let (va, vb) = (read(a), read(b));
    assert!(va > vb + 40, "two instances should differ: {va} vs {vb}");

    // The shared source cooked once, not once per instance.
    assert_eq!(cook.cook_count(src), 1);
}

#[test]
fn animation_crosses_the_component_boundary_in_both_directions() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();

    let noise = add(&mut graph, &reg, root, "noiseTOP", "noise1");
    let comp = darkener(&mut graph, &reg, root, "darken");
    let out = add(&mut graph, &reg, root, "nullTOP", "out1");
    graph.connect(noise, comp, 0).unwrap();
    graph.connect(comp, out, 0).unwrap();
    graph.set_param(noise, "resw", Value::Int(16)).unwrap();
    graph.set_param(noise, "resh", Value::Int(16)).unwrap();

    let mut engines = Engines::new(gpu);
    let mut cook = CookEngine::new();
    let mut time = CookContext::default();
    fn run(
        graph: &Graph,
        out: NodeId,
        engines: &mut Engines,
        cook: &mut CookEngine,
        time: &mut CookContext,
        n: usize,
    ) {
        for _ in 0..n {
            engines.begin_frame();
            cook.cook_frame(graph, &[out], time, engines).unwrap();
            engines.end_frame();
            time.advance(1.0 / 60.0);
        }
    }

    // Static to begin with: everything cooks once and then caches, even
    // across the boundary.
    run(&graph, out, &mut engines, &mut cook, &mut time, 10);
    let inner = graph.find("/darken/level1").unwrap();
    assert_eq!(cook.cook_count(inner), 1, "a static component caches");
    assert_eq!(cook.cook_count(out), 1);

    // Animate the source outside; the inside must go hot.
    graph
        .set_expression(noise, "translate", "absTime * 0.2")
        .unwrap();
    run(&graph, out, &mut engines, &mut cook, &mut time, 10);
    assert_eq!(cook.cook_count(inner), 11, "animation reaches inside");
    assert!(cook.is_time_dependent(out), "and back out again");
}

#[test]
fn a_component_survives_the_project_format() {
    use otd_core::Project;
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let src = add(&mut graph, &reg, root, "constantTOP", "src");
    let comp = darkener(&mut graph, &reg, root, "darken");
    graph.connect(src, comp, 0).unwrap();

    let text = Project::from_graph(&graph, &reg, 60.0).to_ron().unwrap();
    let back = Project::from_ron(&text).unwrap().to_graph(&reg).unwrap();

    let comp2 = back.find("/darken").expect("component survived");
    assert_eq!(
        back.node(comp2).input_labels,
        vec!["in1"],
        "connectors are rebuilt from the In operators inside"
    );
    assert_eq!(
        back.node(comp2).inputs[0],
        back.find("/src"),
        "and the outside wiring reconnected"
    );
    assert!(back.find("/darken/level1").is_some());

    let text2 = Project::from_graph(&back, &reg, 60.0).to_ron().unwrap();
    assert_eq!(text, text2);
}

/// The Phase 3 shape: a component with knobs, used more than once.
#[test]
fn custom_parameters_are_a_components_api() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();

    // A component with one knob, read by the Level inside it.
    let comp = add(&mut graph, &reg, root, "containerCOMP", "tint");
    graph.add_custom_param(
        comp,
        "gain",
        otd_core::Param::float(1.0).with_range(0.0, 4.0),
    );
    let inp = add(&mut graph, &reg, comp, "inTOP", "in1");
    let level = add(&mut graph, &reg, comp, "levelTOP", "level1");
    let outp = add(&mut graph, &reg, comp, "outTOP", "out1");
    graph.connect(inp, level, 0).unwrap();
    graph.connect(level, outp, 0).unwrap();
    graph
        .set_expression(level, "brightness", "parent.gain")
        .unwrap();

    let src = add(&mut graph, &reg, root, "constantTOP", "src");
    graph.set_param(src, "resw", Value::Int(16)).unwrap();
    graph.set_param(src, "resh", Value::Int(16)).unwrap();
    graph
        .set_param(src, "color", Value::Vec4([0.25, 0.25, 0.25, 1.0]))
        .unwrap();
    graph.connect(src, comp, 0).unwrap();

    let mut engines = Engines::new(gpu.clone());
    let mut cook = CookEngine::new();
    let mut time = CookContext::default();

    let render =
        |graph: &Graph, engines: &mut Engines, cook: &mut CookEngine, time: &mut CookContext| {
            engines.begin_frame();
            cook.cook_frame(graph, &[comp], time, engines).unwrap();
            engines.end_frame();
            time.advance(1.0 / 60.0);
            let tex = engines.top.output(graph, comp).unwrap().clone();
            read_pixels_rgba8(&gpu, &tex).unwrap().2[0]
        };

    graph.set_param(comp, "gain", Value::Float(1.0)).unwrap();
    let at_one = render(&graph, &mut engines, &mut cook, &mut time);

    // Turning the component's knob must reach the operator inside it, with
    // no wire between them.
    graph.set_param(comp, "gain", Value::Float(3.0)).unwrap();
    let at_three = render(&graph, &mut engines, &mut cook, &mut time);
    assert!(
        at_three > at_one + 60,
        "the knob did not reach inside: {at_one} then {at_three}"
    );
}

#[test]
fn a_custom_parameter_round_trips_with_its_definition() {
    use otd_core::Project;
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let comp = add(&mut graph, &reg, root, "containerCOMP", "tint");
    graph.add_custom_param(
        comp,
        "gain",
        otd_core::Param::float(2.5)
            .with_label("Gain")
            .with_range(0.0, 4.0),
    );

    let text = Project::from_graph(&graph, &reg, 60.0).to_ron().unwrap();
    let back = Project::from_ron(&text).unwrap().to_graph(&reg).unwrap();
    let comp2 = back.find("/tint").unwrap();
    let p = back
        .node(comp2)
        .param("gain")
        .expect("custom param survived");

    // Nothing else knows this parameter exists, so the file has to carry its
    // whole definition, not just its value.
    assert!(p.custom);
    assert_eq!(p.value, Value::Float(2.5));
    assert_eq!(p.label, "Gain");
    assert_eq!(p.range, Some((0.0, 4.0)));

    let text2 = Project::from_graph(&back, &reg, 60.0).to_ron().unwrap();
    assert_eq!(text, text2);
}

#[test]
fn a_chop_component_works_the_same_way() {
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();

    let comp = add(&mut graph, &reg, root, "containerCOMP", "smooth");
    let inp = add(&mut graph, &reg, comp, "inCHOP", "in1");
    let lag = add(&mut graph, &reg, comp, "lagCHOP", "lag1");
    let outp = add(&mut graph, &reg, comp, "outCHOP", "out1");
    graph.connect(inp, lag, 0).unwrap();
    graph.connect(lag, outp, 0).unwrap();

    // The connector took the In operator's family, so the component now
    // accepts a CHOP and refuses a TOP.
    assert_eq!(
        graph.node(comp).input_families,
        vec![otd_core::Family::Chop]
    );
    let lfo = add(&mut graph, &reg, root, "lfoCHOP", "lfo1");
    let noise = add(&mut graph, &reg, root, "noiseTOP", "noise1");
    assert!(graph.connect(noise, comp, 0).is_err());
    graph
        .connect(lfo, comp, 0)
        .expect("CHOP into a CHOP connector");
    assert_eq!(graph.output_family(comp), Some(otd_core::Family::Chop));
}
