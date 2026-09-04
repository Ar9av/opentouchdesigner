//! The DAT family, including Script DATs running Python inside the cook.

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
    fn data(&self, id: NodeId) -> &otd_engine::DatData {
        self.engines.dat_data(id).expect("the DAT has cooked")
    }
}

#[test]
fn a_table_dat_holds_its_contents_in_the_project() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut rig = Rig::new(gpu);
    let root = rig.graph.root();
    let t = add(&mut rig.graph, &reg, root, "tableDAT", "cues");
    rig.graph
        .set_param(
            t,
            "text",
            Value::Str("name\tlevel\nintro\t0.2\ndrop\t1.0".into()),
        )
        .unwrap();
    rig.run(t);

    let d = rig.data(t);
    assert_eq!(d.num_rows(), 3);
    assert_eq!(d.num_cols(), 2);
    assert_eq!(d.lookup("drop", 1), Some("1.0"));
}

#[test]
fn select_picks_rows_and_columns_by_name() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut rig = Rig::new(gpu);
    let root = rig.graph.root();
    let t = add(&mut rig.graph, &reg, root, "tableDAT", "cues");
    let s = add(&mut rig.graph, &reg, root, "selectDAT", "pick");
    rig.graph.connect(t, s, 0).unwrap();
    rig.graph
        .set_param(
            t,
            "text",
            Value::Str("name\tlevel\tcolour\nintro\t0.2\tblue\ndrop\t1.0\tred".into()),
        )
        .unwrap();
    rig.graph
        .set_param(s, "rows", Value::Str("drop".into()))
        .unwrap();
    rig.graph
        .set_param(s, "cols", Value::Str("colour".into()))
        .unwrap();
    rig.run(s);

    let d = rig.data(s);
    assert_eq!(d.num_rows(), 1);
    assert_eq!(d.cell(0, 0), "red");
}

#[test]
fn json_becomes_a_table_you_can_select_from() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut rig = Rig::new(gpu);
    let root = rig.graph.root();
    let text = add(&mut rig.graph, &reg, root, "textDAT", "payload");
    let json = add(&mut rig.graph, &reg, root, "jsonDAT", "parsed");
    rig.graph.connect(text, json, 0).unwrap();
    rig.graph
        .set_param(
            text,
            "text",
            Value::Str(r#"{"show": {"name": "opener", "bpm": 128}}"#.into()),
        )
        .unwrap();
    rig.run(json);

    let d = rig.data(json);
    assert_eq!(d.lookup("/show/bpm", 1), Some("128"));
    assert_eq!(d.lookup("/show/name", 1), Some("opener"));
}

#[test]
fn malformed_json_reports_itself_rather_than_failing_the_cook() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut rig = Rig::new(gpu);
    let root = rig.graph.root();
    let text = add(&mut rig.graph, &reg, root, "textDAT", "payload");
    let json = add(&mut rig.graph, &reg, root, "jsonDAT", "parsed");
    rig.graph.connect(text, json, 0).unwrap();
    rig.graph
        .set_param(text, "text", Value::Str("{not json".into()))
        .unwrap();
    rig.run(json);

    assert!(
        rig.engines.node_status(&rig.graph, json).is_some(),
        "the node should carry the parse error"
    );
}

#[test]
fn a_script_dat_produces_rows_from_python() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut rig = Rig::new(gpu);
    if rig.engines.python_error().is_some() {
        return;
    }
    let root = rig.graph.root();
    let s = add(&mut rig.graph, &reg, root, "scriptDAT", "gen");
    rig.graph
        .set_param(
            s,
            "source",
            Value::Str("rows = [['i', 'square']] + [[i, i * i] for i in range(4)]".into()),
        )
        .unwrap();
    rig.run(s);

    let d = rig.data(s);
    assert_eq!(d.num_rows(), 5);
    assert_eq!(d.cell(0, 1), "square");
    assert_eq!(d.cell(4, 1), "9");
}

#[test]
fn a_script_dat_can_read_the_network() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut rig = Rig::new(gpu);
    if rig.engines.python_error().is_some() {
        return;
    }
    let root = rig.graph.root();
    let knob = add(&mut rig.graph, &reg, root, "constantCHOP", "knob");
    let s = add(&mut rig.graph, &reg, root, "scriptDAT", "report");
    rig.graph
        .set_param(knob, "value0", Value::Float(0.25))
        .unwrap();
    rig.graph
        .set_param(
            s,
            "source",
            Value::Str("rows = [['level'], [ch('/knob', 'chan1')]]".into()),
        )
        .unwrap();

    // The CHOP has to cook first for the script to see anything, which it
    // does because the path appears in the source.
    rig.run(s);
    assert_eq!(rig.data(s).cell(1, 0), "0.25");
}

#[test]
fn a_broken_script_reports_and_leaves_the_cook_alone() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut rig = Rig::new(gpu);
    if rig.engines.python_error().is_some() {
        return;
    }
    let root = rig.graph.root();
    let s = add(&mut rig.graph, &reg, root, "scriptDAT", "bad");
    rig.graph
        .set_param(s, "source", Value::Str("rows = 1 / 0".into()))
        .unwrap();
    rig.run(s);

    let status = rig.engines.node_status(&rig.graph, s);
    assert!(
        status
            .as_deref()
            .unwrap_or("")
            .contains("ZeroDivisionError"),
        "{status:?}"
    );
}

#[test]
fn a_dat_component_has_data_connectors() {
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let comp = add(&mut graph, &reg, root, "containerCOMP", "sheet");
    let _in = add(&mut graph, &reg, comp, "inDAT", "in1");
    let _out = add(&mut graph, &reg, comp, "outDAT", "out1");
    assert_eq!(graph.node(comp).input_families, vec![otd_core::Family::Dat]);
    assert_eq!(graph.output_family(comp), Some(otd_core::Family::Dat));

    let table = add(&mut graph, &reg, root, "tableDAT", "t");
    graph
        .connect(table, comp, 0)
        .expect("DAT into a DAT connector");
}

#[test]
fn a_table_round_trips_through_the_project_format() {
    use otd_core::Project;
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let t = add(&mut graph, &reg, root, "tableDAT", "cues");
    let contents = "name\tlevel\nintro\t0.2\ndrop\t1.0";
    graph
        .set_param(t, "text", Value::Str(contents.into()))
        .unwrap();

    let text = Project::from_graph(&graph, &reg, 60.0).to_ron().unwrap();
    // A table's contents are project data, so they appear in the diff.
    assert!(text.contains("intro"), "{text}");
    let back = Project::from_ron(&text).unwrap().to_graph(&reg).unwrap();
    assert_eq!(
        back.node(back.find("/cues").unwrap())
            .param("text")
            .unwrap()
            .value
            .as_str(),
        contents
    );
}

#[test]
fn udp_datagrams_arrive_as_rows() {
    use std::net::UdpSocket;

    let gpu = gpu_or_skip!();
    let reg = registry();
    let mut rig = Rig::new(gpu);
    let root = rig.graph.root();
    let udp = add(&mut rig.graph, &reg, root, "udpinDAT", "udpin1");
    // A high port, to avoid colliding with anything a developer is running.
    const PORT: i64 = 38481;
    rig.graph.set_param(udp, "port", Value::Int(PORT)).unwrap();
    rig.run(udp);

    if rig.engines.dat.status_of("/udpin1").is_some() {
        eprintln!("skipping: could not bind UDP port {PORT}");
        return;
    }

    let sender = UdpSocket::bind("127.0.0.1:0").expect("bind sender");
    sender
        .send_to(b"GO cue-12\n", format!("127.0.0.1:{PORT}"))
        .expect("send");

    // The listener is on its own thread; give it a moment, then cook. The
    // node is time dependent, so cooking again with nothing changed in the
    // graph is exactly the path being tested.
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        rig.run(udp);
        if rig.data(udp).num_rows() > 1 {
            break;
        }
    }
    assert_eq!(rig.data(udp).cell(0, 0), "message");
    assert_eq!(
        rig.data(udp).cell(1, 0),
        "GO cue-12",
        "trailing newline trimmed, text intact"
    );
}

#[test]
fn udp_out_sends_changes_and_only_changes() {
    use std::net::UdpSocket;

    let gpu = gpu_or_skip!();
    let reg = registry();

    let listener = UdpSocket::bind("127.0.0.1:0").expect("bind listener");
    listener
        .set_read_timeout(Some(std::time::Duration::from_millis(500)))
        .unwrap();
    let port = listener.local_addr().unwrap().port() as i64;

    let mut rig = Rig::new(gpu);
    let root = rig.graph.root();
    let text = add(&mut rig.graph, &reg, root, "textDAT", "text1");
    let out = add(&mut rig.graph, &reg, root, "udpoutDAT", "udpout1");
    rig.graph.connect(text, out, 0).unwrap();
    rig.graph
        .set_param(text, "text", Value::Str("hello".into()))
        .unwrap();
    rig.graph.set_param(out, "port", Value::Int(port)).unwrap();

    let mut buf = [0u8; 1024];
    rig.run(out);
    let (len, _) = listener.recv_from(&mut buf).expect("first datagram");
    assert_eq!(&buf[..len], b"hello");

    // Cooked again with the same payload: nothing should arrive — UDP Out
    // sends changes, not frames.
    rig.run(out);
    rig.run(out);
    listener
        .set_read_timeout(Some(std::time::Duration::from_millis(200)))
        .unwrap();
    assert!(
        listener.recv_from(&mut buf).is_err(),
        "an unchanged payload was re-sent"
    );

    rig.graph
        .set_param(text, "text", Value::Str("cue 2".into()))
        .unwrap();
    listener
        .set_read_timeout(Some(std::time::Duration::from_millis(500)))
        .unwrap();
    rig.run(out);
    let (len, _) = listener.recv_from(&mut buf).expect("changed datagram");
    assert_eq!(&buf[..len], b"cue 2");
}
