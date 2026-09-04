//! The Replicator: one clone of a master component per row of a table.
//!
//! The property under test is the composition, not just the copying: a
//! Replicator is built out of clone syncing and custom parameters, so a
//! replicant must render *differently* from its siblings when its row says
//! to, and the population must follow the table as rows come and go.

use otd_core::{CookContext, CookEngine, Graph, NodeId, OpRegistry, Value};
use otd_engine::{Engines, registry, replicator};
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

struct Rig {
    graph: Graph,
    reg: OpRegistry,
    engines: Engines,
    cook: CookEngine,
    time: CookContext,
    gpu: GpuContext,
}

impl Rig {
    fn new(gpu: GpuContext) -> Self {
        Rig {
            graph: Graph::new(),
            reg: registry(),
            engines: Engines::new(gpu.clone()),
            cook: CookEngine::new(),
            time: CookContext::default(),
            gpu,
        }
    }

    /// One editor frame: replicate, sync clones, cook.
    fn frame(&mut self, roots: &[NodeId]) -> usize {
        let changed = replicator::sync(&mut self.graph, &self.engines.dats);
        self.graph.sync_clones(&self.reg);
        self.engines.begin_frame();
        self.cook
            .cook_frame(&self.graph, roots, &self.time, &mut self.engines)
            .unwrap();
        self.engines.end_frame();
        self.time.advance(1.0 / 60.0);
        changed
    }

    fn brightness(&self, id: NodeId) -> f64 {
        let tex = self
            .engines
            .top
            .output(&self.graph, id)
            .expect("the TOP has cooked")
            .clone();
        let (_, _, pixels) = read_pixels_rgba8(&self.gpu, &tex).unwrap();
        let sum: u64 = pixels.iter().step_by(4).map(|p| *p as u64).sum();
        sum as f64 / (pixels.len() / 4) as f64
    }
}

/// A master whose one knob is how bright its constant colour is.
fn master(rig: &mut Rig, parent: NodeId) -> NodeId {
    let comp = add(&mut rig.graph, &rig.reg, parent, "containerCOMP", "cell");
    rig.graph
        .add_custom_param(comp, "bright", otd_core::Param::float(0.1));
    let c = add(&mut rig.graph, &rig.reg, comp, "constantTOP", "colour");
    rig.graph
        .set_param(c, "color", Value::Vec4([1.0, 1.0, 1.0, 1.0]))
        .unwrap();
    let level = add(&mut rig.graph, &rig.reg, comp, "levelTOP", "level1");
    let out = add(&mut rig.graph, &rig.reg, comp, "outTOP", "out1");
    rig.graph.connect(c, level, 0).unwrap();
    rig.graph.connect(level, out, 0).unwrap();
    rig.graph
        .set_expression(level, "brightness", "parent.bright")
        .unwrap();
    comp
}

#[test]
fn the_population_follows_the_table_and_each_row_sets_its_parameters() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let root = rig.graph.root();
    let _master = master(&mut rig, root);

    let table = add(&mut rig.graph, &rig.reg, root, "tableDAT", "rows");
    rig.graph
        .set_param(
            table,
            "text",
            Value::Str("name\tbright\ndim\t0.2\nfull\t1.0\n".into()),
        )
        .unwrap();

    let rep = add(&mut rig.graph, &rig.reg, root, "replicatorCOMP", "rep1");
    rig.graph
        .set_param(rep, "master", Value::Str("/cell".into()))
        .unwrap();
    rig.graph
        .set_param(rep, "template", Value::Str("/rows".into()))
        .unwrap();

    let changed = rig.frame(&[]);
    assert_eq!(changed, 2, "two rows, two replicants");

    let dim = rig.graph.find("/rep1/dim").expect("replicant from row 1");
    let full = rig.graph.find("/rep1/full").expect("replicant from row 2");

    // The clone sync filled the replicants from the master, and the row set
    // the knob: same network, different pictures.
    let dim_out = rig.graph.find("/rep1/dim/out1").unwrap();
    let full_out = rig.graph.find("/rep1/full/out1").unwrap();
    rig.frame(&[dim_out, full_out]);
    let a = rig.brightness(dim_out);
    let b = rig.brightness(full_out);
    assert!(
        b > a * 3.0,
        "rows should render differently: dim={a:.1} full={b:.1}"
    );
    assert_eq!(
        rig.graph.node(dim).param("bright").unwrap().value,
        Value::Float(0.2)
    );
    assert_eq!(
        rig.graph.node(full).param("bright").unwrap().value,
        Value::Float(1.0)
    );

    // Steady state: the same table must not keep editing the graph, or every
    // frame would dirty every replicant.
    assert_eq!(rig.frame(&[]), 0);

    // A removed row removes its replicant; the survivor keeps its identity.
    rig.graph
        .set_param(
            table,
            "text",
            Value::Str("name\tbright\nfull\t1.0\n".into()),
        )
        .unwrap();
    rig.frame(&[]);
    assert!(rig.graph.find("/rep1/dim").is_none(), "row gone, node gone");
    assert_eq!(rig.graph.find("/rep1/full"), Some(full));

    // And a hand-made node inside the replicator is not the replicator's to
    // delete.
    let mine = add(&mut rig.graph, &rig.reg, rep, "noiseTOP", "mine");
    rig.frame(&[]);
    assert_eq!(rig.graph.find("/rep1/mine"), Some(mine));
}

#[test]
fn a_replicator_without_headers_numbers_its_items() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let root = rig.graph.root();
    let _master = master(&mut rig, root);

    let table = add(&mut rig.graph, &rig.reg, root, "tableDAT", "rows");
    rig.graph
        .set_param(table, "text", Value::Str("a\nb\nc\n".into()))
        .unwrap();
    let rep = add(&mut rig.graph, &rig.reg, root, "replicatorCOMP", "rep1");
    rig.graph
        .set_param(rep, "master", Value::Str("/cell".into()))
        .unwrap();
    rig.graph
        .set_param(rep, "template", Value::Str("/rows".into()))
        .unwrap();
    rig.graph
        .set_param(rep, "byname", Value::Bool(false))
        .unwrap();

    rig.frame(&[]);
    for name in ["/rep1/item1", "/rep1/item2", "/rep1/item3"] {
        assert!(rig.graph.find(name).is_some(), "missing {name}");
    }
}
