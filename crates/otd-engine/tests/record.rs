//! Recording a TOP to a movie file.
//!
//! The assertion that matters is not "a file appeared" — an empty file appears
//! too, and so does one ffmpeg refused to finalise. It is that the file plays,
//! has the frames that were cooked, and has the size they were cooked at.

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

/// Recording is the one feature that cannot be faked without the encoder.
macro_rules! ffmpeg_or_skip {
    () => {
        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_err()
        {
            eprintln!("skipping: ffmpeg is not installed");
            return;
        }
    };
}

struct Rig {
    graph: Graph,
    reg: OpRegistry,
    engines: Engines,
    cook: CookEngine,
    time: CookContext,
}

impl Rig {
    fn new(gpu: GpuContext) -> Self {
        Rig {
            graph: Graph::new(),
            reg: registry(),
            engines: Engines::new(gpu),
            cook: CookEngine::new(),
            time: CookContext::default(),
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
    fn run(&mut self, root: NodeId, frames: usize) {
        for _ in 0..frames {
            self.engines.begin_frame();
            self.cook
                .cook_frame(&self.graph, &[root], &self.time, &mut self.engines)
                .unwrap();
            self.engines.end_frame();
            self.time.advance(1.0 / 60.0);
        }
    }
}

/// Ask ffprobe what actually landed on disk.
fn probe(path: &std::path::Path) -> Option<(u32, u32, u64)> {
    let out = std::process::Command::new("ffprobe")
        .args(["-v", "error"])
        .args(["-select_streams", "v:0"])
        .args(["-count_frames"])
        .args([
            "-show_entries",
            "stream=width,height,nb_read_frames",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut parts = text.trim().split(',');
    Some((
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ))
}

#[test]
fn a_recording_lands_on_disk_and_plays_back() {
    let gpu = gpu_or_skip!();
    ffmpeg_or_skip!();
    let dir = std::env::temp_dir().join("otd-record-test");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("take1.mp4");
    let _ = std::fs::remove_file(&path);

    let mut rig = Rig::new(gpu);
    let noise = rig.add("noiseTOP", "noise1");
    let out = rig.add("moviefileoutTOP", "rec1");
    rig.graph.connect(noise, out, 0).unwrap();
    rig.set(noise, "resw", Value::Int(128));
    rig.set(noise, "resh", Value::Int(64));
    rig.set(out, "file", Value::Str(path.to_string_lossy().into()));
    rig.set(out, "fps", Value::Float(30.0));

    // Not recording yet: the node is a pass-through and writes nothing.
    rig.run(out, 3);
    assert!(
        !path.exists(),
        "nothing should be written before Record is on"
    );

    rig.set(out, "record", Value::Bool(true));
    rig.run(out, 20);

    // Turning Record off is what closes the file — the encoder needs its
    // stdin shut to write the container's trailer.
    rig.set(out, "record", Value::Bool(false));
    rig.run(out, 1);

    let (w, h, frames) = probe(&path).expect("ffprobe could not read the file");
    assert_eq!((w, h), (128, 64), "recorded at the input's resolution");
    assert!(
        frames >= 18,
        "expected about 20 frames, the file has {frames}"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_movie_out_passes_its_input_through_untouched() {
    // It sits in the middle of a chain, so anything downstream has to see the
    // picture rather than a black frame or nothing at all.
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let constant = rig.add("constantTOP", "grey");
    let out = rig.add("moviefileoutTOP", "rec1");
    rig.graph.connect(constant, out, 0).unwrap();
    rig.set(constant, "resw", Value::Int(32));
    rig.set(constant, "resh", Value::Int(32));
    rig.set(constant, "color", Value::Vec4([0.25, 0.5, 0.75, 1.0]));
    rig.run(out, 2);

    let tex = rig
        .engines
        .top
        .output(&rig.graph, out)
        .expect("the node produced a texture")
        .clone();
    assert_eq!((tex.key.width, tex.key.height), (32, 32));
    let (_, _, px) = otd_gpu::read_pixels_rgba8(rig.engines.top.context(), &tex).unwrap();
    assert!((px[0] as i32 - 64).abs() < 3, "red was {}", px[0]);
    assert!((px[1] as i32 - 128).abs() < 3, "green was {}", px[1]);
}

#[test]
fn a_bad_destination_is_reported_on_the_node_rather_than_failing_the_cook() {
    // Losing the render because a path is wrong is exactly the failure mode
    // the rest of the engine avoids for missing devices and missing movies.
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let constant = rig.add("constantTOP", "grey");
    let out = rig.add("moviefileoutTOP", "rec1");
    rig.graph.connect(constant, out, 0).unwrap();
    rig.set(
        out,
        "file",
        Value::Str("/no/such/folder/anywhere/take.mp4".into()),
    );
    rig.set(out, "record", Value::Bool(true));
    rig.run(out, 2);

    let status = rig
        .engines
        .node_status(&rig.graph, out)
        .expect("the node should say what went wrong");
    assert!(
        status.contains("no such folder"),
        "unhelpful message: {status}"
    );
    // And the picture is still there.
    assert!(rig.engines.top.output(&rig.graph, out).is_some());
}
