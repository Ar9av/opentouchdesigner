//! Movie File In and Video Device In, against real files.
//!
//! The claim worth testing is not "it decodes" but "the picture on the wire
//! is the picture in the file, at the time the timeline asked for". So the
//! fixtures are built here with known colours per frame, and the assertions
//! read pixels back off the GPU.

use otd_core::{CookContext, CookEngine, Graph, NodeId, OpRegistry, Value};
use otd_gpu::{GpuContext, TopEngine, ops, read_pixels_rgba8};

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

fn have_ffmpeg() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// A scratch directory of this test's own, named so parallel tests cannot
/// collide.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("otd-video-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Rig {
    graph: Graph,
    reg: OpRegistry,
    engine: TopEngine,
    cook: CookEngine,
    time: CookContext,
    gpu: GpuContext,
}

impl Rig {
    fn new(gpu: GpuContext) -> Self {
        Rig {
            graph: Graph::new(),
            reg: ops::registry(),
            engine: TopEngine::new(gpu.clone()),
            cook: CookEngine::new(),
            time: CookContext::default(),
            gpu,
        }
    }

    fn add(&mut self, op: &str, name: &str) -> NodeId {
        let root = self.graph.root();
        let def = self.reg.get(op).unwrap().clone();
        self.graph.create(root, &def, Some(name)).unwrap()
    }

    /// One frame. The frame counter advances because the cook engine
    /// resolves each node once per frame — a loop that never advances it
    /// cooks once, which is correct behaviour and a trap for a harness.
    fn run(&mut self, root: NodeId) {
        self.engine.begin_frame();
        self.cook
            .cook_frame(&self.graph, &[root], &self.time, &mut self.engine)
            .unwrap();
        self.engine.end_frame();
        self.time.frame += 1;
    }

    /// Cook until the decoder thread has produced something, or give up.
    /// Decoding happens off the cook thread by design, so "the first frame
    /// is not here yet" is a normal state for a frame or two.
    fn run_until_picture(&mut self, root: NodeId) -> bool {
        for _ in 0..200 {
            self.run(root);
            if self.max_channel(root) > 4 {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        false
    }

    fn pixels(&self, id: NodeId) -> (u32, u32, Vec<u8>) {
        let tex = self.engine.output(&self.graph, id).expect("cooked").clone();
        read_pixels_rgba8(&self.gpu, &tex).unwrap()
    }

    fn max_channel(&self, id: NodeId) -> u8 {
        let (_, _, p) = self.pixels(id);
        p.iter().copied().max().unwrap_or(0)
    }

    /// Average red, green and blue over the whole picture.
    fn average_rgb(&self, id: NodeId) -> (f64, f64, f64) {
        let (_, _, p) = self.pixels(id);
        let n = (p.len() / 4) as f64;
        let mut sums = [0f64; 3];
        for px in p.chunks_exact(4) {
            for c in 0..3 {
                sums[c] += px[c] as f64;
            }
        }
        (sums[0] / n, sums[1] / n, sums[2] / n)
    }

    fn at(&self, id: NodeId, x: u32, y: u32) -> [u8; 4] {
        let (w, _, p) = self.pixels(id);
        let i = ((y * w + x) * 4) as usize;
        [p[i], p[i + 1], p[i + 2], p[i + 3]]
    }
}

#[test]
fn a_still_image_reaches_the_texture_at_its_own_size() {
    let gpu = gpu_or_skip!();
    let dir = scratch("still");
    let path = dir.join("halves.png");

    // Left half pure red, right half pure blue — position and colour are
    // both checkable, so a flipped or swizzled upload cannot pass.
    let mut buffer = image::RgbaImage::new(64, 32);
    for (x, _y, px) in buffer.enumerate_pixels_mut() {
        *px = if x < 32 {
            image::Rgba([255, 0, 0, 255])
        } else {
            image::Rgba([0, 0, 255, 255])
        };
    }
    buffer.save(&path).unwrap();

    let mut rig = Rig::new(gpu);
    let movie = rig.add(ops::MOVIE_IN, "movie1");
    rig.graph
        .set_param(movie, "file", Value::Str(path.display().to_string()))
        .unwrap();

    assert!(
        rig.run_until_picture(movie),
        "the still never arrived: {:?}",
        rig.engine.status(movie)
    );

    // The picture's own size wins over the fallback resolution parameters.
    let (w, h, _) = rig.pixels(movie);
    assert_eq!((w, h), (64, 32));

    let left = rig.at(movie, 8, 16);
    let right = rig.at(movie, 56, 16);
    assert!(
        left[0] > 200 && left[2] < 40,
        "left half should be red, got {left:?}"
    );
    assert!(
        right[2] > 200 && right[0] < 40,
        "right half should be blue, got {right:?}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_missing_file_reports_itself_and_still_produces_a_texture() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let movie = rig.add(ops::MOVIE_IN, "movie1");
    rig.graph
        .set_param(movie, "file", Value::Str("/definitely/not/here.mp4".into()))
        .unwrap();
    rig.run(movie);

    // Losing a source mid-show must not stop the render: a message on the
    // node, and black rather than nothing.
    let status = rig.engine.status(movie).unwrap_or_default().to_string();
    assert!(
        status.contains("not/here.mp4") || status.contains("ffmpeg"),
        "unhelpful message: {status:?}"
    );
    let (w, h, _) = rig.pixels(movie);
    assert_eq!((w, h), (1280, 720), "falls back to the parameter size");
    assert_eq!(rig.max_channel(movie), 0, "black, not garbage");
}

#[test]
fn an_empty_file_parameter_is_not_an_error() {
    let gpu = gpu_or_skip!();
    let mut rig = Rig::new(gpu);
    let movie = rig.add(ops::MOVIE_IN, "movie1");
    rig.run(movie);
    // A freshly created node has nothing to play. That is a blank node, not
    // a broken one — it must not paint the canvas red.
    assert!(rig.engine.status(movie).is_none());
    assert_eq!(rig.max_channel(movie), 0);
}

/// Build a three-second clip: one second red, one green, one blue. Scrubbing
/// then has an unambiguous right answer at every time.
fn build_rgb_clip(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("rgb.mp4");
    let status = std::process::Command::new("ffmpeg")
        .args(["-y", "-v", "error"])
        .args(["-f", "lavfi", "-i", "color=c=red:s=64x64:d=1:r=30"])
        .args(["-f", "lavfi", "-i", "color=c=lime:s=64x64:d=1:r=30"])
        .args(["-f", "lavfi", "-i", "color=c=blue:s=64x64:d=1:r=30"])
        .args([
            "-filter_complex",
            "[0:v][1:v][2:v]concat=n=3:v=1:a=0[out]",
            "-map",
            "[out]",
        ])
        // yuv420p so this is a normal H.264 file rather than an exotic one.
        .args(["-pix_fmt", "yuv420p", "-c:v", "libx264"])
        .arg(&path)
        .status()
        .expect("run ffmpeg");
    assert!(status.success(), "could not build the test clip");
    path
}

#[test]
fn a_movie_plays_and_the_timeline_decides_which_frame() {
    let gpu = gpu_or_skip!();
    if !have_ffmpeg() {
        eprintln!("skipping: ffmpeg is not installed");
        return;
    }
    let dir = scratch("clip");
    let path = build_rgb_clip(&dir);

    let mut rig = Rig::new(gpu);
    let movie = rig.add(ops::MOVIE_IN, "movie1");
    rig.graph
        .set_param(movie, "file", Value::Str(path.display().to_string()))
        .unwrap();

    assert!(
        rig.run_until_picture(movie),
        "no frame decoded: {:?}",
        rig.engine.status(movie)
    );
    // The movie's own size, not the fallback.
    let (w, h, _) = rig.pixels(movie);
    assert_eq!((w, h), (64, 64));

    // Second 0 is red. Compression means "red" is not exactly 255,0,0, so
    // the assertion is on which channel dominates — which is the thing that
    // would break if frames were mistimed or channels swapped.
    let (r, g, b) = rig.average_rgb(movie);
    assert!(
        r > g + 40.0 && r > b + 40.0,
        "t=0 should be red: {r:.0} {g:.0} {b:.0}"
    );

    // Jump to the middle second: green. Playback follows the timeline, so
    // moving the playhead moves the picture — this is also the seek path,
    // since the jump is backwards-compatible but far from where we are.
    rig.time.time = 1.5;
    assert!(rig.run_until_green(movie), "t=1.5 never became green");

    // And back to the start, which can only work by seeking a pipe that
    // cannot itself seek.
    rig.time.time = 0.1;
    let mut red_again = false;
    for _ in 0..200 {
        rig.run(movie);
        let (r, g, b) = rig.average_rgb(movie);
        if r > g + 40.0 && r > b + 40.0 {
            red_again = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(red_again, "scrubbing backwards did not re-seek");

    std::fs::remove_dir_all(&dir).ok();
}

impl Rig {
    fn run_until_green(&mut self, id: NodeId) -> bool {
        for _ in 0..200 {
            self.run(id);
            let (r, g, b) = self.average_rgb(id);
            if g > r + 40.0 && g > b + 40.0 {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        false
    }
}

#[test]
fn a_movie_node_releases_its_decoder_when_it_goes_away() {
    let gpu = gpu_or_skip!();
    if !have_ffmpeg() {
        eprintln!("skipping: ffmpeg is not installed");
        return;
    }
    let dir = scratch("release");
    let path = build_rgb_clip(&dir);

    let mut rig = Rig::new(gpu);
    let movie = rig.add(ops::MOVIE_IN, "movie1");
    rig.graph
        .set_param(movie, "file", Value::Str(path.display().to_string()))
        .unwrap();
    assert!(rig.run_until_picture(movie));

    // Deleting the node has to stop the subprocess — a patch edited for an
    // hour must not leave an hour of ffmpeg processes behind. `forget` is
    // what the editor calls, and dropping the decoder is what kills it.
    rig.engine.forget(movie);
    assert!(rig.engine.output(&rig.graph, movie).is_none());

    std::fs::remove_dir_all(&dir).ok();
}
