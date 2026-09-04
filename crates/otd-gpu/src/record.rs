//! Writing a TOP to a movie file.
//!
//! The mirror of `video.rs`, and the same bargain PLAN.md §3 sanctions: an
//! **ffmpeg subprocess**, fed raw RGBA on stdin. It costs a readback and a
//! memcpy per frame where a hardware encoder session would not, and in
//! exchange it works on all three platforms today, encodes anything ffmpeg
//! encodes, and adds nothing to what it takes to *compile* OpenTouchDesigner.
//!
//! Two decisions worth stating, because both are visible in use.
//!
//! **The readback happens after the frame is submitted**, in `end_frame`, not
//! during the cook. Reading during the cook would copy a texture whose passes
//! are still sitting in an unsubmitted encoder — that is fine for a TOP to
//! CHOP, which only wants numbers and says it is a frame behind, and it is not
//! fine for a recording, where a frame behind means the file does not match
//! what the artist watched.
//!
//! **A full queue blocks the frame** rather than dropping. A dropped frame in
//! a recording is not a stutter you can see and forgive; it is a file whose
//! timing is silently wrong, and it stays wrong forever. Recording is an
//! explicit act with an obvious cost, so the honest failure is a slower editor
//! while it runs.

use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};

/// How many frames may be in flight to the encoder.
///
/// Small on purpose: it is enough to ride out a slow disk write without
/// letting a 4K recording quietly take a gigabyte of RAM.
const QUEUE_DEPTH: usize = 8;

/// A running recording.
pub struct Recorder {
    /// What this was opened for — path, size and rate. A change to any of
    /// them has to start a new file rather than write mismatched frames into
    /// the old one.
    pub key: String,
    pub width: u32,
    pub height: u32,
    frames: Option<SyncSender<Vec<u8>>>,
    stop: Arc<AtomicBool>,
    child: Arc<ChildHandle>,
    written: Arc<AtomicU64>,
    problem: Arc<std::sync::Mutex<Option<String>>>,
}

struct ChildHandle(std::sync::Mutex<Option<Child>>);

impl ChildHandle {
    /// Let ffmpeg finish: closing stdin is what makes it write the container's
    /// index and trailer. Killing it here would leave an unplayable file,
    /// which is the one outcome a recording must not have.
    fn finish(&self) {
        if let Ok(mut slot) = self.0.lock() {
            if let Some(mut child) = slot.take() {
                let _ = child.wait();
            }
        }
    }
}

impl Recorder {
    /// Start ffmpeg and the thread that feeds it.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        key: String,
        path: &str,
        width: u32,
        height: u32,
        fps: f64,
        quality: u32,
        codec: &str,
    ) -> Result<Recorder, String> {
        if width == 0 || height == 0 {
            return Err("nothing to record: the input has no picture".into());
        }
        if path.trim().is_empty() {
            return Err("set a File to record to".into());
        }
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                return Err(format!("no such folder: {}", parent.display()));
            }
        }

        let mut command = Command::new("ffmpeg");
        command
            .arg("-y")
            .args(["-f", "rawvideo"])
            .args(["-pix_fmt", "rgba"])
            .args(["-s", &format!("{width}x{height}")])
            .args(["-r", &format!("{:.6}", fps.clamp(1.0, 240.0))])
            .args(["-i", "-"])
            .args(["-an"])
            .args(["-c:v", codec]);

        if codec == "prores_ks" {
            // ProRes wants a profile, and 4444 is the one that keeps alpha —
            // the reason to pick ProRes over h264 in the first place.
            command
                .args(["-profile:v", "4"])
                .args(["-pix_fmt", "yuva444p10le"]);
        } else {
            // Quality is exposed as 0..100 because CRF's "lower is better,
            // 18-28 is the useful band" is a thing you have to be told.
            let crf = 51 - (quality.min(100) as f64 * 0.51).round() as i32;
            command
                .args(["-crf", &crf.to_string()])
                .args(["-preset", "veryfast"])
                // h264 needs even dimensions, and a patch at 1281 wide is not
                // an error the artist should have to hear about.
                .args(["-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2"])
                .args(["-pix_fmt", "yuv420p"]);
        }

        let mut child = command
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => {
                    "ffmpeg is not installed — `brew install ffmpeg`".to_string()
                }
                _ => format!("could not start ffmpeg: {e}"),
            })?;

        let mut stdin = child.stdin.take().ok_or("ffmpeg took no input pipe")?;
        let problem: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));

        // ffmpeg says why it refused on stderr — a codec the build lacks, a
        // path it cannot write. That belongs on the node, not in a terminal
        // nobody is watching.
        if let Some(mut stderr) = child.stderr.take() {
            let sink = problem.clone();
            let _ = std::thread::Builder::new()
                .name("otd-record-stderr".into())
                .spawn(move || {
                    use std::io::Read as _;
                    let mut text = String::new();
                    let _ = stderr.read_to_string(&mut text);
                    // Only the last line: ffmpeg's banner is not a problem
                    // report and burying the real message under it helps
                    // nobody.
                    if let Some(last) = text.lines().rev().find(|l| !l.trim().is_empty()) {
                        if let Ok(mut slot) = sink.lock() {
                            *slot = Some(last.trim().to_string());
                        }
                    }
                });
        }

        let (tx, rx) = sync_channel::<Vec<u8>>(QUEUE_DEPTH);
        let stop = Arc::new(AtomicBool::new(false));
        let written = Arc::new(AtomicU64::new(0));
        let child = Arc::new(ChildHandle(std::sync::Mutex::new(Some(child))));

        let worker_stop = stop.clone();
        let worker_written = written.clone();
        let worker_problem = problem.clone();
        let _ = std::thread::Builder::new()
            .name("otd-record".into())
            .spawn(move || {
                for frame in rx {
                    if worker_stop.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Err(e) = stdin.write_all(&frame) {
                        if let Ok(mut slot) = worker_problem.lock() {
                            *slot = Some(format!("the encoder stopped accepting frames: {e}"));
                        }
                        break;
                    }
                    worker_written.fetch_add(1, Ordering::Relaxed);
                }
                // Dropping stdin closes the pipe, which is ffmpeg's cue to
                // finalise the file.
                drop(stdin);
            });

        Ok(Recorder {
            key,
            width,
            height,
            frames: Some(tx),
            stop,
            child,
            written,
            problem,
        })
    }

    /// Hand one frame to the encoder, blocking if it is behind.
    pub fn push(&self, pixels: Vec<u8>) {
        if let Some(tx) = &self.frames {
            let _ = tx.send(pixels);
        }
    }

    pub fn frames_written(&self) -> u64 {
        self.written.load(Ordering::Relaxed)
    }

    pub fn problem(&self) -> Option<String> {
        self.problem.lock().ok().and_then(|p| p.clone())
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(false, Ordering::Relaxed);
        // Closing the sender ends the worker's loop, which drops stdin, which
        // lets ffmpeg write its trailer. Then wait for it: a `Drop` that
        // returned before the file was closed would make "stop recording and
        // open the file" a race the artist loses intermittently.
        self.frames = None;
        self.child.finish();
    }
}
