//! Execute DATs — the callback layer.
//!
//! What is actually being asserted is the *phasing*, not that Python runs.
//! A callback reads a graph that is not moving under it, writes through a
//! queue, and the write lands between frames. That is the only arrangement in
//! which a script can change the network without being able to corrupt a cook
//! halfway through one.

use otd_core::{CookContext, CookEngine, Graph, NodeId, OpRegistry, Value};
use otd_engine::{Engines, registry};
use otd_gpu::GpuContext;

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

struct Rig {
    graph: Graph,
    reg: OpRegistry,
    engines: Engines,
    cook: CookEngine,
    time: CookContext,
    /// Problems reported when the callbacks' edits were applied.
    problems: Vec<String>,
}

impl Rig {
    fn new(gpu: GpuContext) -> Self {
        Rig {
            graph: Graph::new(),
            reg: registry(),
            engines: Engines::new(gpu),
            cook: CookEngine::new(),
            time: CookContext::default(),
            problems: Vec::new(),
        }
    }
    fn add(&mut self, op: &str, name: &str) -> NodeId {
        let root = self.graph.root();
        let def = self.reg.get(op).unwrap_or_else(|| panic!("{op}")).clone();
        self.graph.create(root, &def, Some(name)).unwrap()
    }
    fn set(&mut self, id: NodeId, key: &str, v: Value) {
        self.graph.set_param(id, key, v).unwrap();
    }
    /// One frame, the way a host runs one: cook, then apply what the
    /// callbacks asked for.
    fn frame(&mut self) {
        let roots = otd_engine::execute::roots(&self.graph);
        self.engines.begin_frame();
        self.cook
            .cook_frame(&self.graph, &roots, &self.time, &mut self.engines)
            .unwrap();
        self.engines.end_frame();
        let edits = self.engines.take_edits();
        let (_, problems) = otd_core::edit::apply(&mut self.graph, &edits);
        self.problems = problems;
        self.time.advance(1.0 / 60.0);
    }
    fn param(&self, id: NodeId, key: &str) -> f64 {
        self.graph
            .node(id)
            .param(key)
            .map(|p| p.eval(&otd_core::EvalContext::default()).as_f64())
            .unwrap_or(f64::NAN)
    }
    fn status(&self, id: NodeId) -> Option<String> {
        self.engines.node_status(&self.graph, id)
    }
}

/// Python may not have started on this machine; skip rather than fail, the
/// same bargain the GPU tests make.
fn python_or_skip(rig: &Rig) -> bool {
    match rig.engines.python_error() {
        Some(e) => {
            eprintln!("skipping: Python unavailable ({e})");
            false
        }
        None => true,
    }
}

#[test]
fn an_execute_dat_is_a_cook_root_without_being_flagged_one() {
    // The trap this avoids: a callback that silently does nothing until you
    // find the render checkbox.
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let ex = rig.add("executeDAT", "exec1");
    assert!(!rig.graph.node(ex).flags.render);
    assert_eq!(otd_engine::execute::roots(&rig.graph), vec![ex]);
}

#[test]
fn a_callback_can_set_a_parameter_and_it_lands_next_frame() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    if !python_or_skip(&rig) {
        return;
    }
    let blur = rig.add("blurTOP", "blur1");
    let ex = rig.add("executeDAT", "exec1");
    rig.set(
        ex,
        "source",
        Value::Str("def onFrameStart(frame):\n    setpar('/blur1', 'size', 4 + frame)\n".into()),
    );

    let before = rig.param(blur, "size");
    rig.frame();
    assert!(rig.problems.is_empty(), "{:?}", rig.problems);
    let after = rig.param(blur, "size");
    assert_ne!(before, after, "the callback should have set Size");
    assert_eq!(after, 4.0, "frame 0: 4 + 0");

    rig.frame();
    assert_eq!(rig.param(blur, "size"), 5.0, "frame 1: 4 + 1");
}

#[test]
fn on_start_runs_once_and_not_once_a_frame() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    if !python_or_skip(&rig) {
        return;
    }
    let blur = rig.add("blurTOP", "blur1");
    let ex = rig.add("executeDAT", "exec1");
    // The namespace persists between frames, so a counter is the honest way
    // to ask "how many times did this run".
    rig.set(
        ex,
        "source",
        Value::Str(
            "runs = 0\n\
             def onStart():\n    \
                 global runs\n    \
                 runs += 1\n    \
                 setpar('/blur1', 'size', runs)\n"
                .into(),
        ),
    );

    for _ in 0..5 {
        rig.frame();
    }
    assert_eq!(rig.param(blur, "size"), 1.0, "onStart fired more than once");
}

#[test]
fn a_chop_execute_fires_on_change_and_on_the_edge() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    if !python_or_skip(&rig) {
        return;
    }
    let src = rig.add("constantCHOP", "src");
    let changes = rig.add("blurTOP", "changes");
    let edges = rig.add("blurTOP", "edges");
    let ex = rig.add("chopexecuteDAT", "watch1");
    rig.set(ex, "chop", Value::Str("/src".into()));
    rig.set(ex, "threshold", Value::Float(0.5));
    rig.set(
        ex,
        "source",
        Value::Str(
            "changes = 0\n\
             edges = 0\n\
             def onValueChange(channel, value, prev):\n    \
                 global changes\n    \
                 changes += 1\n    \
                 setpar('/changes', 'size', changes)\n\
             def onOffToOn(channel, value):\n    \
                 global edges\n    \
                 edges += 1\n    \
                 setpar('/edges', 'size', edges)\n"
                .into(),
        ),
    );

    // First frame establishes the value: a change, but no edge, because there
    // was nothing to cross from.
    rig.frame();
    assert_eq!(rig.param(changes, "size"), 1.0);
    assert_eq!(rig.param(edges, "size"), 8.0, "no edge on the first sight");

    // An unchanged channel fires nothing at all.
    rig.frame();
    rig.frame();
    assert_eq!(rig.param(changes, "size"), 1.0, "a still channel is silent");

    // Cross the threshold upwards.
    rig.set(src, "value0", Value::Float(1.0));
    rig.frame();
    assert_eq!(rig.param(changes, "size"), 2.0);
    assert_eq!(rig.param(edges, "size"), 1.0, "the rising edge fired");

    // Back down: a change, and not a rising edge.
    rig.set(src, "value0", Value::Float(0.0));
    rig.frame();
    assert_eq!(rig.param(changes, "size"), 3.0);
    assert_eq!(rig.param(edges, "size"), 1.0);
}

#[test]
fn a_broken_callback_reports_itself_and_keeps_the_frame() {
    // The rule the whole engine follows: a typo during a show must not stop
    // the render.
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    if !python_or_skip(&rig) {
        return;
    }
    let ex = rig.add("executeDAT", "exec1");
    rig.set(
        ex,
        "source",
        Value::Str("def onFrameStart(frame):\n    1 / 0\n".into()),
    );
    rig.frame();

    let status = rig.status(ex).expect("the node should say what broke");
    assert!(
        status.contains("ZeroDivisionError"),
        "unhelpful message: {status}"
    );
    // And the next frame still runs.
    rig.frame();
}

#[test]
fn an_edit_to_a_node_that_is_gone_is_reported_not_fatal() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    if !python_or_skip(&rig) {
        return;
    }
    let ex = rig.add("executeDAT", "exec1");
    rig.set(
        ex,
        "source",
        Value::Str("def onFrameStart(frame):\n    setpar('/nope', 'size', 1)\n".into()),
    );
    rig.frame();

    assert_eq!(rig.problems.len(), 1);
    assert!(rig.problems[0].contains("/nope"), "{:?}", rig.problems);
}

#[test]
fn an_inactive_execute_dat_fires_nothing() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    if !python_or_skip(&rig) {
        return;
    }
    let blur = rig.add("blurTOP", "blur1");
    let ex = rig.add("executeDAT", "exec1");
    rig.set(
        ex,
        "source",
        Value::Str("def onFrameStart(frame):\n    setpar('/blur1', 'size', 99)\n".into()),
    );
    rig.set(ex, "active", Value::Bool(false));
    let before = rig.param(blur, "size");
    rig.frame();
    assert_eq!(rig.param(blur, "size"), before);
}

// ------------------------------------------------------------------ panels

#[test]
fn a_panel_chop_reads_widget_values_as_channels() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let fader = rig.add("sliderCOMP", "fader1");
    let go = rig.add("buttonCOMP", "go");
    let panel = rig.add("panelCHOP", "panel1");
    rig.set(panel, "ops", Value::Str("/fader1 /go".into()));
    rig.set(fader, "value", Value::Float(0.75));
    rig.set(go, "value", Value::Float(1.0));

    let roots = vec![panel];
    rig.engines.begin_frame();
    rig.cook
        .cook_frame(&rig.graph, &roots, &rig.time, &mut rig.engines)
        .unwrap();
    rig.engines.end_frame();

    let data = rig.engines.chop_data(panel).expect("the CHOP cooked");
    assert_eq!(data.names(), vec!["fader1", "go"]);
    assert_eq!(data.value("fader1"), Some(0.75));
    assert_eq!(data.value("go"), Some(1.0));
}

#[test]
fn a_widgets_state_is_a_parameter_so_a_callback_can_move_it() {
    // The claim the panel module makes: a widget is driven by the network as
    // readily as it drives it, because there is no second value system.
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    if !python_or_skip(&rig) {
        return;
    }
    let fader = rig.add("sliderCOMP", "fader1");
    let ex = rig.add("executeDAT", "exec1");
    rig.set(
        ex,
        "source",
        Value::Str("def onFrameStart(frame):\n    setpar('/fader1', 'value', 0.42)\n".into()),
    );
    rig.frame();

    assert_eq!(rig.param(fader, "value"), 0.42);
    // And the editor sees the moved fader, because it reads the same place.
    let w = &otd_engine::panel::widgets(&rig.graph)[0];
    assert_eq!(w.value, 0.42);
}
