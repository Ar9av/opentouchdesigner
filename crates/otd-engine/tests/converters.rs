//! The converter operators — the sanctioned way to cross a wire family.
//!
//! PLAN.md §2.1 keeps the families apart and then says the bridge is "explicit
//! converter ops + parameter references". These are the first half. What is
//! being asserted here is not really the arithmetic; it is that a wire between
//! two families cooks in the right order, carries the right data, and is
//! rejected everywhere it has not been declared.

use otd_core::{CookContext, CookEngine, Graph, GraphError, NodeId, OpRegistry, Value};
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

fn add(graph: &mut Graph, reg: &OpRegistry, parent: NodeId, op: &str, name: &str) -> NodeId {
    let def = reg.get(op).unwrap().clone();
    graph.create(parent, &def, Some(name)).unwrap()
}

struct Rig {
    graph: Graph,
    engines: Engines,
    cook: CookEngine,
    time: CookContext,
}

impl Rig {
    fn new(gpu: GpuContext) -> Self {
        Rig {
            graph: Graph::new(),
            engines: Engines::new(gpu),
            cook: CookEngine::new(),
            time: CookContext::default(),
        }
    }
    fn run(&mut self, root: NodeId) {
        self.engines.begin_frame();
        self.cook
            .cook_frame(&self.graph, &[root], &self.time, &mut self.engines)
            .unwrap();
        self.engines.end_frame();
        self.time.advance(1.0 / 60.0);
    }
    fn chan(&self, id: NodeId, name: &str) -> Vec<f32> {
        self.engines
            .chop_data(id)
            .expect("the CHOP has cooked")
            .channels
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no channel `{name}`"))
            .samples
            .clone()
    }
}

#[test]
fn a_table_becomes_channels_named_by_its_header() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut rig = Rig::new(gpu);
    let root = rig.graph.root();
    let table = add(&mut rig.graph, &reg, root, "tableDAT", "cues");
    let to_chop = add(&mut rig.graph, &reg, root, "dattochopCHOP", "vals");
    rig.graph.connect(table, to_chop, 0).unwrap();
    rig.graph
        .set_param(
            table,
            "text",
            Value::Str("level\tspeed\n0.2\t1.5\n1.0\t2.5\n0.5\t0.25".into()),
        )
        .unwrap();
    rig.run(to_chop);

    assert_eq!(rig.chan(to_chop, "level"), vec![0.2, 1.0, 0.5]);
    assert_eq!(rig.chan(to_chop, "speed"), vec![1.5, 2.5, 0.25]);
}

#[test]
fn a_label_column_reads_as_zero_rather_than_failing_the_cook() {
    // A cue table almost always has a name column. Refusing to cook because
    // "intro" is not a number would make the ordinary case the broken one.
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut rig = Rig::new(gpu);
    let root = rig.graph.root();
    let table = add(&mut rig.graph, &reg, root, "tableDAT", "cues");
    let to_chop = add(&mut rig.graph, &reg, root, "dattochopCHOP", "vals");
    rig.graph.connect(table, to_chop, 0).unwrap();
    rig.graph
        .set_param(
            table,
            "text",
            Value::Str("name\tlevel\nintro\t0.2\ndrop\t1.0".into()),
        )
        .unwrap();
    rig.run(to_chop);

    assert_eq!(rig.chan(to_chop, "name"), vec![0.0, 0.0]);
    assert_eq!(rig.chan(to_chop, "level"), vec![0.2, 1.0]);
}

#[test]
fn channels_become_a_table_and_survive_the_round_trip() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut rig = Rig::new(gpu);
    let root = rig.graph.root();
    let table = add(&mut rig.graph, &reg, root, "tableDAT", "src");
    let to_chop = add(&mut rig.graph, &reg, root, "dattochopCHOP", "chans");
    let back = add(&mut rig.graph, &reg, root, "choptodatDAT", "again");
    rig.graph.connect(table, to_chop, 0).unwrap();
    rig.graph.connect(to_chop, back, 0).unwrap();
    rig.graph
        .set_param(table, "text", Value::Str("a\tb\n1\t2\n3\t4".into()))
        .unwrap();
    rig.run(back);

    let d = rig.engines.dat_data(back).unwrap();
    assert_eq!(d.rows[0], vec!["a".to_string(), "b".to_string()]);
    assert_eq!(d.rows[1], vec!["1".to_string(), "2".to_string()]);
    assert_eq!(d.rows[2], vec!["3".to_string(), "4".to_string()]);
}

#[test]
fn geometry_points_become_one_sample_each() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut rig = Rig::new(gpu);
    let root = rig.graph.root();
    let grid = add(&mut rig.graph, &reg, root, "gridSOP", "grid");
    let to_chop = add(&mut rig.graph, &reg, root, "soptochopCHOP", "pts");
    rig.graph.connect(grid, to_chop, 0).unwrap();
    rig.graph
        .set_param(to_chop, "attrs", Value::Str("P uv".into()))
        .unwrap();
    rig.run(to_chop);

    let points = rig.engines.geometry_of(grid).unwrap().points.len();
    assert!(points > 0);
    for name in ["Px", "Py", "Pz", "uvx", "uvy"] {
        assert_eq!(
            rig.chan(to_chop, name).len(),
            points,
            "`{name}` should have one sample per point"
        );
    }
    // A grid is flat in Z, which is the cheap way to know the right
    // component landed in the right channel.
    assert!(rig.chan(to_chop, "Pz").iter().all(|v| *v == 0.0));
    assert!(rig.chan(to_chop, "Px").iter().any(|v| *v != 0.0));
}

#[test]
fn channels_become_a_texture_a_shader_can_sample() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut rig = Rig::new(gpu);
    let root = rig.graph.root();
    let pattern = add(&mut rig.graph, &reg, root, "patternCHOP", "wave");
    let to_top = add(&mut rig.graph, &reg, root, "choptotopTOP", "tex");
    rig.graph.connect(pattern, to_top, 0).unwrap();
    rig.graph
        .set_param(pattern, "length", Value::Int(64))
        .unwrap();
    rig.run(to_top);

    let tex = rig.engines.top.output(&rig.graph, to_top).unwrap();
    // One column per sample, one row per channel.
    assert_eq!(tex.key.width, 64);
    assert_eq!(tex.key.height, 1);

    // And the pixels are the samples, not a picture of them. A default
    // Pattern CHOP ramps 0..1, so the two ends pin the mapping down.
    let ctx = rig.engines.top.context();
    let (_, _, rgba) = otd_gpu::read_pixels_rgba_f32(ctx, tex).unwrap();
    let samples = rig.chan(pattern, "chan1");
    assert!((rgba[0] - samples[0]).abs() < 1e-3);
    let last = (63 * 4) as usize;
    assert!((rgba[last] - samples[63]).abs() < 1e-3);
}

#[test]
fn a_top_reads_back_as_channels_one_frame_behind() {
    // The delay is not a bug to be fixed later: this frame's passes are still
    // in an unsubmitted encoder, and waiting for them would stall the frame
    // on the render it is in the middle of building. TouchDesigner's TOP to
    // CHOP is one frame behind for exactly this reason.
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut rig = Rig::new(gpu);
    let root = rig.graph.root();
    let constant = add(&mut rig.graph, &reg, root, "constantTOP", "grey");
    let to_chop = add(&mut rig.graph, &reg, root, "toptochopCHOP", "read");
    rig.graph.connect(constant, to_chop, 0).unwrap();
    for (key, v) in [("resw", 4), ("resh", 4)] {
        rig.graph.set_param(constant, key, Value::Int(v)).unwrap();
    }
    rig.graph
        .set_param(constant, "color", Value::Vec4([0.25, 0.5, 0.75, 1.0]))
        .unwrap();
    rig.graph
        .set_param(to_chop, "layout", Value::Str("average".into()))
        .unwrap();

    rig.run(to_chop);
    rig.run(to_chop);

    let r = rig.chan(to_chop, "r");
    let g = rig.chan(to_chop, "g");
    assert_eq!(r.len(), 1);
    // 16-bit float, so exact equality is the wrong assertion to make.
    assert!((r[0] - 0.25).abs() < 1e-3, "r was {}", r[0]);
    assert!((g[0] - 0.5).abs() < 1e-3, "g was {}", g[0]);
}

#[test]
fn a_wire_only_crosses_where_the_operator_declared_it() {
    // The whole point of typed families is that the mistake is caught at
    // connect time. A converter widens exactly one input, not the graph.
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut rig = Rig::new(gpu);
    let root = rig.graph.root();
    let table = add(&mut rig.graph, &reg, root, "tableDAT", "cues");
    let lag = add(&mut rig.graph, &reg, root, "lagCHOP", "smooth");
    let to_chop = add(&mut rig.graph, &reg, root, "dattochopCHOP", "vals");

    assert!(matches!(
        rig.graph.connect(table, lag, 0),
        Err(GraphError::FamilyMismatch("DAT", "CHOP"))
    ));
    assert!(rig.graph.connect(table, to_chop, 0).is_ok());
    // And the converter's output is an ordinary CHOP.
    assert!(rig.graph.connect(to_chop, lag, 0).is_ok());
}
