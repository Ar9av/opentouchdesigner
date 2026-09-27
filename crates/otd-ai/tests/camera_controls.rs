//! Check the actual behavior of recipes whose success is not "it moves".
use otd_ai::{patch, recipes};
use otd_core::{CookContext, CookEngine, Graph, NodeId, Value};
use otd_engine::{Engines, registry};
use otd_gpu::{GpuContext, read_pixels_rgba8};

struct Scene {
    gpu: GpuContext,
    graph: Graph,
    engine: Engines,
    cook: CookEngine,
    time: CookContext,
    source: NodeId,
    out: NodeId,
}

impl Scene {
    fn new(recipe: &str, audio_stub: bool) -> Self {
        let gpu = GpuContext::headless().expect("camera recipe tests require a GPU");
        let reg = registry();
        let mut graph = Graph::new();
        let root = graph.root();
        let source = graph.create(root, reg.get("constantTOP").unwrap(), Some("source1")).unwrap();
        graph.set_param(source, "resw", Value::Int(32)).unwrap();
        graph.set_param(source, "resh", Value::Int(32)).unwrap();
        graph.set_param(source, "color", Value::Vec4([0.2, 0.2, 0.2, 1.0])).unwrap();
        let mut plan = recipes::find(recipe).unwrap().plan(&reg).unwrap();
        if audio_stub {
            let mic = plan.nodes.iter_mut().find(|n| n.name == "mic1").unwrap();
            mic.op = "constantCHOP".into();
            mic.params.insert("name".into(), Value::Str("chan".into()));
            mic.params.insert("value0".into(), Value::Float(0.0));
        }
        let (applied, out) = patch::apply(&mut graph, root, &reg, &plan).unwrap();
        assert!(applied.warnings.is_empty(), "{:?}", applied.warnings);
        Self { engine: Engines::new(gpu.clone()), gpu, graph, cook: CookEngine::new(),
            time: CookContext::default(), source, out: out.unwrap() }
    }

    fn step(&mut self) -> Vec<u8> {
        self.engine.begin_frame();
        self.cook.cook_frame(&mut self.graph, &[self.out], &self.time, &mut self.engine).unwrap();
        self.engine.end_frame();
        self.time.advance(1.0 / 60.0);
        let tex = self.engine.top.output(&self.graph, self.out).unwrap();
        read_pixels_rgba8(&self.gpu, tex).unwrap().2
    }

    fn colour(&mut self, colour: [f64; 4]) {
        self.graph.set_param(self.source, "color", Value::Vec4(colour)).unwrap();
    }
}

#[test]
fn held_key_freezes_exact_pixels_and_release_resumes() {
    let mut scene = Scene::new("freeze", false);
    let initial = scene.step();
    scene.engine.set_input_state(otd_chop::InputState { keys: vec!["f".into()], ..Default::default() });
    scene.colour([0.8, 0.1, 0.2, 1.0]);
    for _ in 0..8 {
        assert_eq!(scene.step(), initial, "holding F must preserve the captured frame");
    }
    scene.engine.set_input_state(Default::default());
    assert_ne!(scene.step(), initial, "releasing F must return to the live image");
}

#[test]
fn audio_envelope_changes_output_and_silence_has_a_visible_baseline() {
    let mut scene = Scene::new("audiocamera", true);
    let mut silent = vec![];
    for _ in 0..60 { silent = scene.step(); }
    assert!(silent[0] > 20, "silence must leave the camera visible");
    let mic = scene.graph.find("/mic1").unwrap();
    scene.graph.set_param(mic, "value0", Value::Float(0.8)).unwrap();
    let mut loud = vec![];
    for _ in 0..60 { loud = scene.step(); }
    assert!(loud[0] > silent[0] + 10, "audio must visibly change brightness");
    assert!(loud[0] < 100, "bounded gain must prevent runaway feedback");
}

#[test]
fn green_screen_is_replaced_but_red_foreground_is_preserved() {
    let mut scene = Scene::new("greenscreen", false);
    scene.colour([0.0, 1.0, 0.0, 1.0]);
    let keyed = scene.step();
    assert!(keyed[1] < keyed[2], "green backdrop must reveal the purple portal");
    scene.colour([1.0, 0.0, 0.0, 1.0]);
    let foreground = scene.step();
    assert!(foreground[0] > 240 && foreground[1] < 10 && foreground[2] < 10,
        "opaque red foreground must remain visible over the portal");
}

#[test]
fn blob_tracker_exports_valid_coordinates_and_clears_on_empty_input() {
    let mut scene = Scene::new("blobfollow", false);
    scene.colour([1.0, 1.0, 1.0, 1.0]);
    for _ in 0..3 { scene.step(); }
    let read = |scene: &Scene, channel: &str| {
        scene.engine.channel_value(&scene.graph, "/tracker1", channel).unwrap()
    };
    assert_eq!(read(&scene, "present"), 1.0);
    assert!((read(&scene, "x") - 0.5).abs() < 0.01);
    assert!((read(&scene, "y") - 0.5).abs() < 0.01);
    assert!((read(&scene, "area") - 1.0).abs() < 0.01);
    scene.colour([0.0, 0.0, 0.0, 1.0]);
    for _ in 0..3 { scene.step(); }
    assert_eq!(read(&scene, "present"), 0.0);
    assert_eq!(read(&scene, "vx"), 0.0);
    assert_eq!(read(&scene, "vy"), 0.0);
}
