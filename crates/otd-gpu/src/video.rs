//! Video and image input: Movie File In and Video Device In.
//!
//! PLAN.md Phase 1 asks for these on GStreamer. GStreamer is a *system*
//! dependency rather than a crate — it has to be installed, versioned and
//! shipped per platform — and a build that cannot be compiled or verified is
//! not worth writing. §3 of the plan already sanctions the alternative:
//! "ffmpeg subprocess for offline export". This module takes that seriously
//! as the transport for input too.
//!
//! So there are two paths, chosen by what the file is:
//!
//!  * **Still images** — PNG, JPEG, WebP, BMP, TGA, TIFF — are decoded
//!    in-process by the `image` crate. No external tool, no way for it to be
//!    missing, and no subprocess for the case where you just want one picture.
//!  * **Everything that moves** — mp4, mov, mkv, webm, avi, gif — and every
//!    camera runs through an `ffmpeg` subprocess piping raw RGBA on stdout.
//!    If ffmpeg is not installed the node says so, by name, and produces
//!    black rather than failing the cook.
//!
//! What this trades away against GStreamer is in-process zero-copy hardware
//! decode: a pipe costs a memcpy per frame and an upload. What it buys is
//! that it works on all three platforms today, with no build-time dependency
//! and nothing to install to *compile* OpenTouchDesigner.
//!
//! Two rules from the rest of the codebase apply unchanged. Decoding happens
//! on its own thread and hands finished frames to the cook, so nothing here
//! can block a frame. And a missing file, missing camera or missing ffmpeg
//! degrades to a message on the node, because losing a source mid-show must
//! not stop the render.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};

/// One decoded frame, RGBA8, tightly packed.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// How far ahead it is still cheaper to decode than to seek. Beyond this the
/// reader restarts ffmpeg at the wanted time instead of grinding forward.
const SEEK_AHEAD_LIMIT: i64 = 90;

/// Frames the decoder may run ahead of the cook. Small on purpose: this is
/// the back-pressure that stops a 60 fps source filling memory when the
/// patch is running at 30, and for a camera it is the latency budget.
const QUEUE_DEPTH: usize = 2;

/// Extensions decoded in-process, without ffmpeg.
const STILL_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "bmp", "tga", "tif", "tiff", "ico", "ppm", "pnm",
];

pub fn is_still(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| STILL_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// What a source is worth knowing before the first frame arrives.
#[derive(Clone, Debug, Default)]
pub struct Info {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// Seconds. Zero for a camera, and for anything ffprobe would not say.
    pub duration: f64,
}

// -------------------------------------------------------- finding the tools
//
// The lookup itself lives in `otd_core::tools`: the movie node, the audio
// file CHOP and the assistant's CLIs all have the same problem with a
// Finder-launched `PATH`, and it was worth solving once.

fn ffmpeg_command() -> Option<Command> {
    otd_core::tools::ffmpeg().map(Command::new)
}

fn ffprobe_command() -> Option<Command> {
    otd_core::tools::ffprobe().map(Command::new)
}

/// Whether both tools were found. Lets a caller say "ffmpeg is missing"
/// rather than guessing at why a file would not open.
pub fn tools_installed() -> bool {
    otd_core::tools::media_tools_installed()
}

/// One wording for the one thing the user has to do about it.
pub fn missing_ffmpeg() -> String {
    otd_core::tools::missing_ffmpeg()
}

// ------------------------------------------------------------------ probing

/// Ask ffprobe what a file is. Returns `None` when ffprobe is missing or the
/// file is not media — the caller turns that into a message on the node.
pub fn probe(path: &str) -> Option<Info> {
    let out = ffprobe_command()?
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,r_frame_rate:format=duration",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let stream = json.get("streams")?.get(0)?;
    let width = stream.get("width")?.as_u64()? as u32;
    let height = stream.get("height")?.as_u64()? as u32;
    let fps = stream
        .get("r_frame_rate")
        .and_then(|v| v.as_str())
        .and_then(parse_rational)
        .filter(|f| *f > 0.0)
        .unwrap_or(30.0);
    let duration = json
        .get("format")
        .and_then(|f| f.get("duration"))
        .and_then(|d| d.as_str())
        .and_then(|d| d.parse::<f64>().ok())
        .unwrap_or(0.0);
    Some(Info {
        width,
        height,
        fps,
        duration,
    })
}

/// ffprobe reports frame rates as `30000/1001`, not as a decimal.
fn parse_rational(text: &str) -> Option<f64> {
    match text.split_once('/') {
        Some((n, d)) => {
            let (n, d) = (n.trim().parse::<f64>().ok()?, d.trim().parse::<f64>().ok()?);
            (d != 0.0).then_some(n / d)
        }
        None => text.trim().parse().ok(),
    }
}

/// The capture input ffmpeg uses for a camera on this platform.
pub fn camera_input() -> (&'static str, &'static str) {
    if cfg!(target_os = "macos") {
        ("avfoundation", "0")
    } else if cfg!(target_os = "windows") {
        ("dshow", "video=default")
    } else {
        ("v4l2", "/dev/video0")
    }
}

/// One thing a camera can actually do.
#[derive(Clone, Debug, PartialEq)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    pub rates: Vec<f64>,
}

/// A line of ffmpeg's supported-mode list: `  1280x720@[15.000000 30.000000]fps`,
/// usually behind a `[in#0 @ 0x7f…]` prefix.
fn parse_mode_line(line: &str) -> Option<Mode> {
    // Drop any bracketed logging prefix without disturbing the `@[...]` that
    // carries the rates.
    let body = match line.rfind("] ") {
        Some(i) if line.starts_with('[') => &line[i + 2..],
        _ => line,
    };
    let (size, rest) = body.trim().split_once('@')?;
    let (w, h) = size.trim().split_once('x')?;
    let width = w.trim().parse().ok()?;
    let height = h.trim().parse().ok()?;
    let rates: Vec<f64> = rest
        .trim_start_matches('[')
        .split(']')
        .next()?
        .split_whitespace()
        .filter_map(|r| r.parse().ok())
        .collect();
    (width > 0 && height > 0).then_some(Mode {
        width,
        height,
        rates,
    })
}

/// Ask a capture device what it can do.
///
/// There is no query for this, so the question is asked by requesting
/// something impossible: ffmpeg refuses and prints the list. It costs a third
/// of a second and no camera warm-up, which is far cheaper than opening the
/// device several times to find out by trial.
pub fn camera_modes(format: &str, device: &str) -> Vec<Mode> {
    let Some(mut ffmpeg) = ffmpeg_command() else {
        return Vec::new();
    };
    let out = ffmpeg
        .args(["-hide_banner", "-v", "error", "-f", format])
        .args(["-video_size", "1x1", "-framerate", "1"])
        .args(["-i", device, "-frames:v", "1", "-f", "null", "-"])
        .stdin(Stdio::null())
        .output();
    let Ok(out) = out else { return Vec::new() };
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .filter_map(parse_mode_line)
        .collect()
}

/// The mode closest to what the patch asked for, and a rate the device has.
///
/// "Closest" is by pixel count rather than by width: asking for 1280×720 on a
/// portrait camera should not land on 1080×1920 just because a width matches.
fn choose_mode(modes: &[Mode], want_w: u32, want_h: u32, want_fps: f64) -> Option<(Mode, f64)> {
    let want_pixels = (want_w as i64) * (want_h as i64);
    let best = modes.iter().min_by_key(|m| {
        let pixels = (m.width as i64) * (m.height as i64);
        (pixels - want_pixels).abs()
    })?;
    let rate = best
        .rates
        .iter()
        .copied()
        .min_by(|a, b| {
            (a - want_fps)
                .abs()
                .partial_cmp(&(b - want_fps).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(want_fps);
    Some((best.clone(), rate))
}

// ------------------------------------------------------------------ decoding

/// A running decode: a thread, and a short queue of frames it has produced.
struct Reader {
    frames: Receiver<(i64, Arc<Frame>)>,
    stop: Arc<AtomicBool>,
    /// Kept so the pipe can be killed rather than waited on.
    child: Option<Arc<SharedChild>>,
}

/// A child process that can be killed from the owning thread while the
/// reader thread is blocked on its stdout.
struct SharedChild(std::sync::Mutex<Child>);

impl SharedChild {
    fn kill(&self) {
        if let Ok(mut c) = self.0.lock() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Killing the process is what unblocks the thread's read; without it
        // a paused camera would keep a thread alive for the session.
        if let Some(child) = &self.child {
            child.kill();
        }
    }
}

/// A source being played: what it was opened with, what it is, and the
/// decode currently running.
pub struct Source {
    /// The file path or device index this was opened for, so a changed
    /// parameter reopens rather than quietly keeping the old picture.
    pub key: String,
    pub info: Info,
    pub live: bool,
    reader: Option<Reader>,
    /// The index carried by the newest frame taken off the queue.
    current: i64,
    frame: Option<Arc<Frame>>,
    /// Bumped whenever a different frame is adopted, so the engine uploads
    /// to the GPU only when there is something new to upload.
    pub revision: u64,
    /// A forward seek has been issued and the new stream has not produced a
    /// frame yet.
    ///
    /// Starting an ffmpeg costs about a tenth of a second, and asking for
    /// playback faster than the machine decodes means the playhead has run on
    /// again by the time it is ready — so the next cook seeks again, killing
    /// the process that was about to deliver. Repeat, and the picture sticks
    /// on whatever frame arrived last: at speed 20 it froze ten seconds in.
    ///
    /// Counting cooks was the wrong lever, because `otd render` runs flat out
    /// and twenty-four of its cooks can be less than one process start. What
    /// matters is not how long it has been but whether the last seek has
    /// *landed*, which is this.
    awaiting_seek: bool,
    /// Set by a worker that could not do its job, and read by the cook.
    problem: Arc<std::sync::Mutex<Option<String>>>,
}

impl Source {
    /// Open a still image: one frame, decoded once, on its own thread.
    pub fn still(key: String, path: &Path) -> Source {
        let (tx, frames) = sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        // The decode happens off the cook thread, so its failure has to
        // travel back rather than being thrown away — an unreadable image
        // that reports nothing is indistinguishable from one still loading.
        let problem: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
        let sink = problem.clone();
        let path = path.to_path_buf();
        let _ = std::thread::Builder::new()
            .name("otd-image-decode".into())
            .spawn(move || match decode_still(&path) {
                Ok(frame) => {
                    let _ = tx.send((0, Arc::new(frame)));
                }
                Err(e) => {
                    if let Ok(mut slot) = sink.lock() {
                        *slot = Some(e);
                    }
                }
            });
        Source {
            key,
            // Filled in from the frame itself when it arrives — an image
            // needs no probe.
            info: Info::default(),
            live: false,
            reader: Some(Reader {
                frames,
                stop,
                child: None,
            }),
            current: -1,
            frame: None,
            revision: 0,
            awaiting_seek: false,
            problem,
        }
    }

    /// Open a media file through ffmpeg, starting at `start_index`.
    pub fn file(key: String, path: &str, info: Info, start_index: i64) -> Result<Source, String> {
        let mut source = Source {
            key,
            info,
            live: false,
            reader: None,
            current: -1,
            frame: None,
            revision: 0,
            awaiting_seek: false,
            problem: Default::default(),
        };
        source.reader = Some(source.spawn_file(path, start_index)?);
        source.current = start_index - 1;
        Ok(source)
    }

    /// Open a camera. Always live: there is no seeking a present moment.
    ///
    /// A capture device only does the modes it does — AVFoundation refuses a
    /// size, rate or pixel format it has not got, rather than picking
    /// something near — so the requested resolution is negotiated down to a
    /// real one rather than passed through and left to fail.
    pub fn camera(
        key: String,
        device: &str,
        width: u32,
        height: u32,
        fps: f64,
    ) -> Result<Source, String> {
        let (format, default_device) = camera_input();
        let device = if device.trim().is_empty() {
            default_device.to_string()
        } else {
            device.trim().to_string()
        };

        let modes = camera_modes(format, &device);
        let (chosen_w, chosen_h, chosen_fps) = match choose_mode(&modes, width, height, fps) {
            Some((mode, rate)) => (mode.width, mode.height, rate),
            // Nothing reported: v4l2 and dshow do not print a mode list, so
            // pass the request through and let ffmpeg answer for it.
            None => (width, height, fps),
        };

        let mut command = ffmpeg_command().ok_or_else(missing_ffmpeg)?;
        command.args(["-hide_banner", "-v", "error", "-f", format]);
        // AVFoundation's default of yuv420p is not one of the formats a Mac
        // camera offers; uyvy422 is what they all do.
        if format == "avfoundation" {
            command.args(["-pix_fmt", "uyvy422"]);
        }
        command
            .args(["-framerate", &format!("{chosen_fps}")])
            .args(["-video_size", &format!("{chosen_w}x{chosen_h}")])
            .args(["-i", &device]);
        let problem: Arc<std::sync::Mutex<Option<String>>> = Default::default();
        let reader = spawn_ffmpeg(command, chosen_w, chosen_h, 0, problem.clone())?;
        Ok(Source {
            key,
            info: Info {
                width: chosen_w,
                height: chosen_h,
                fps: chosen_fps,
                duration: 0.0,
            },
            live: true,
            reader: Some(reader),
            current: -1,
            frame: None,
            revision: 0,
            awaiting_seek: false,
            problem,
        })
    }

    fn spawn_file(&self, path: &str, start_index: i64) -> Result<Reader, String> {
        let mut command = ffmpeg_command().ok_or_else(missing_ffmpeg)?;
        command.args(["-v", "error"]);
        if start_index > 0 {
            // Before -i, so ffmpeg seeks the container rather than decoding
            // and discarding everything up to the seek point.
            command.args(["-ss", &format!("{}", start_index as f64 / self.info.fps)]);
        }
        command
            .args(["-i", path])
            // A fixed output rate makes frame index and time the same fact,
            // which is what lets a seek be computed rather than searched for.
            .args(["-r", &format!("{}", self.info.fps)]);
        spawn_ffmpeg(
            command,
            self.info.width,
            self.info.height,
            start_index,
            self.problem.clone(),
        )
    }

    /// The frame to show, given the index the timeline is asking for.
    ///
    /// `wanted` is ignored for a live source, which always shows the newest
    /// frame that has arrived.
    pub fn advance(&mut self, path: &str, wanted: i64) -> Option<&Arc<Frame>> {
        let mut adopted = false;

        // Take everything waiting. For a live source that means the newest
        // frame wins and latency stays at one frame; for a file it means
        // catching up after a slow frame costs no extra frames of lag.
        while let Some(reader) = &self.reader {
            match reader.frames.try_recv() {
                Ok((index, frame)) => {
                    self.current = index;
                    self.frame = Some(frame);
                    adopted = true;
                    if !self.live && index >= wanted {
                        break;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    // The stream ended. For a file that is the last frame,
                    // and the caller decides whether to loop.
                    self.reader = None;
                    break;
                }
            }
        }

        if adopted {
            // Whatever we were waiting for has arrived.
            self.awaiting_seek = false;
            self.revision += 1;
            if self.info.width == 0 {
                if let Some(f) = &self.frame {
                    self.info.width = f.width;
                    self.info.height = f.height;
                }
            }
        }

        // A file whose playhead moved backwards, or jumped far ahead, is
        // cheaper to re-open at the new time than to decode up to.
        if !self.live && self.info.fps > 0.0 {
            let backwards = wanted < self.current;
            let far_ahead = wanted > self.current + SEEK_AHEAD_LIMIT;
            // A backward jump is somebody scrubbing and must feel immediate,
            // so it preempts a pending seek. A forward one waits for the last
            // seek to land, or playback faster than this machine decodes
            // spends all its time starting processes it then kills.
            if backwards || (far_ahead && !self.awaiting_seek) {
                if let Ok(reader) = self.spawn_file(path, wanted.max(0)) {
                    self.reader = Some(reader);
                    self.current = wanted.max(0) - 1;
                    self.awaiting_seek = true;
                }
            }
        }

        self.frame.as_ref()
    }

    /// Whether the decode has finished and nothing more will arrive.
    pub fn ended(&self) -> bool {
        self.reader.is_none()
    }

    pub fn frame(&self) -> Option<&Arc<Frame>> {
        self.frame.as_ref()
    }

    /// What went wrong on the decode thread, if anything did.
    pub fn problem(&self) -> Option<String> {
        self.problem.lock().ok().and_then(|p| p.clone())
    }
}

/// Start ffmpeg writing raw RGBA to stdout and a thread reading frames off it.
fn spawn_ffmpeg(
    mut command: Command,
    width: u32,
    height: u32,
    start_index: i64,
    problem: Arc<std::sync::Mutex<Option<String>>>,
) -> Result<Reader, String> {
    if width == 0 || height == 0 {
        return Err("source has no picture size".into());
    }
    let mut child = command
        .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdout(Stdio::piped())
        // Kept rather than discarded: when a capture device refuses a mode,
        // ffmpeg's own words list the ones it would accept, and that belongs
        // on the node where somebody can read it.
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                "ffmpeg is not installed — `brew install ffmpeg`, or use a PNG/JPEG still".into()
            }
            _ => format!("could not start ffmpeg: {e}"),
        })?;

    let mut stdout = child
        .stdout
        .take()
        .ok_or("ffmpeg produced no output pipe")?;
    if let Some(mut stderr) = child.stderr.take() {
        let sink = problem.clone();
        let _ = std::thread::Builder::new()
            .name("otd-video-stderr".into())
            .spawn(move || {
                let mut text = String::new();
                use std::io::Read as _;
                let _ = stderr.read_to_string(&mut text);
                let text = text.trim();
                if !text.is_empty() {
                    if let Ok(mut slot) = sink.lock() {
                        *slot = Some(text.to_string());
                    }
                }
            });
    }
    let child = Arc::new(SharedChild(std::sync::Mutex::new(child)));
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, frames): (SyncSender<(i64, Arc<Frame>)>, _) = sync_channel(QUEUE_DEPTH);

    let worker_stop = stop.clone();
    let _ = std::thread::Builder::new()
        .name("otd-video-decode".into())
        .spawn(move || {
            let bytes = width as usize * height as usize * 4;
            let mut index = start_index;
            let mut buffer = vec![0u8; bytes];
            while !worker_stop.load(Ordering::Relaxed) {
                if stdout.read_exact(&mut buffer).is_err() {
                    break; // End of stream, or the process was killed.
                }
                let frame = Arc::new(Frame {
                    width,
                    height,
                    pixels: buffer.clone(),
                });
                // Block when the cook is behind: back-pressure is what keeps
                // a fast source from filling memory. Only a disconnect ends
                // the loop — a full queue is normal.
                let mut pending = (index, frame);
                loop {
                    match tx.try_send(pending) {
                        Ok(()) => break,
                        Err(TrySendError::Full(back)) => {
                            pending = back;
                            if worker_stop.load(Ordering::Relaxed) {
                                return;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(2));
                        }
                        Err(TrySendError::Disconnected(_)) => return,
                    }
                }
                index += 1;
            }
        });

    Ok(Reader {
        frames,
        stop,
        child: Some(child),
    })
}

/// Decode a still image to RGBA8.
fn decode_still(path: &Path) -> Result<Frame, String> {
    let image = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let rgba = image.to_rgba8();
    Ok(Frame {
        width: rgba.width(),
        height: rgba.height(),
        pixels: rgba.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_rates_are_read_as_the_rationals_ffprobe_reports() {
        // 29.97 is `30000/1001`, and getting this wrong drifts a minute of
        // footage by two frames.
        assert!((parse_rational("30000/1001").unwrap() - 29.97).abs() < 0.01);
        assert_eq!(parse_rational("25/1"), Some(25.0));
        assert_eq!(parse_rational("60"), Some(60.0));
        // A still image reports 0/0; it must not become an infinity.
        assert_eq!(parse_rational("0/0"), None);
        assert_eq!(parse_rational("nonsense"), None);
    }

    #[test]
    fn stills_are_decoded_in_process_and_movies_are_not() {
        assert!(is_still("/shots/plate.PNG"));
        assert!(is_still("bg.jpeg"));
        assert!(!is_still("clip.mp4"));
        // A GIF moves, so it goes through ffmpeg like any other video.
        assert!(!is_still("loop.gif"));
        assert!(!is_still("no-extension"));
    }

    /// Exactly what a MacBook Pro camera printed, prefix and all. Keeping
    /// the real strings is the point: this parser exists because there is no
    /// query for a device's modes, only ffmpeg's refusal to use a bad one.
    const REAL_MODE_LIST: &str = "\
[in#0 @ 0x79a810000] Selected video size (1x1) is not supported by the device.
[in#0 @ 0x79a810000] Supported modes:
[in#0 @ 0x79a810000]   640x480@[15.000000 30.000000]fps
[in#0 @ 0x79a810000]   1280x720@[15.000000 30.000000]fps
[in#0 @ 0x79a810000]   1920x1080@[15.000000 30.000000]fps
[in#0 @ 0x79a810000]   1080x1920@[15.000000 30.000000]fps
[in#0 @ 0x79a80c000] Error opening input: Input/output error";

    #[test]
    fn a_devices_modes_are_read_out_of_ffmpegs_refusal() {
        let modes: Vec<Mode> = REAL_MODE_LIST.lines().filter_map(parse_mode_line).collect();
        assert_eq!(modes.len(), 4, "{modes:?}");
        assert_eq!(modes[1].width, 1280);
        assert_eq!(modes[1].height, 720);
        assert_eq!(modes[1].rates, vec![15.0, 30.0]);
        // The surrounding prose must not become a mode.
        assert!(parse_mode_line("[in#0 @ 0x1] Supported modes:").is_none());
        assert!(parse_mode_line("Error opening input: Input/output error").is_none());
    }

    #[test]
    fn the_nearest_real_mode_is_chosen_by_pixel_count() {
        let modes: Vec<Mode> = REAL_MODE_LIST.lines().filter_map(parse_mode_line).collect();

        // Exactly available: taken as asked.
        let (mode, rate) = choose_mode(&modes, 1280, 720, 30.0).unwrap();
        assert_eq!((mode.width, mode.height, rate), (1280, 720, 30.0));

        // Not available: the nearest by area. Asking for 640x1080 does *not*
        // land on 640x480 just because the widths agree — 1280x720 is much
        // closer to the picture that was asked for.
        let (mode, _) = choose_mode(&modes, 640, 1080, 30.0).unwrap();
        assert_eq!((mode.width, mode.height), (1280, 720));

        // And an in-between request rounds to the nearer of its neighbours.
        let (mode, _) = choose_mode(&modes, 1280, 1024, 30.0).unwrap();
        assert_eq!((mode.width, mode.height), (1280, 720));

        // A rate the device has not got becomes the closest one it has,
        // because AVFoundation refuses rather than resamples.
        let (_, rate) = choose_mode(&modes, 640, 480, 60.0).unwrap();
        assert_eq!(rate, 30.0);
        let (_, rate) = choose_mode(&modes, 640, 480, 12.0).unwrap();
        assert_eq!(rate, 15.0);

        // A device that reports nothing — v4l2, dshow — leaves the request
        // alone for ffmpeg to answer for.
        assert!(choose_mode(&[], 1280, 720, 30.0).is_none());
    }

    #[test]
    fn a_still_decodes_to_rgba() {
        // Written and read back here, so the test owns both sides.
        let dir = std::env::temp_dir().join(format!("otd-still-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dot.png");
        let mut buffer = image::RgbaImage::new(2, 1);
        buffer.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        buffer.put_pixel(1, 0, image::Rgba([0, 255, 0, 128]));
        buffer.save(&path).unwrap();

        let frame = decode_still(&path).unwrap();
        assert_eq!((frame.width, frame.height), (2, 1));
        assert_eq!(&frame.pixels[..4], &[255, 0, 0, 255]);
        assert_eq!(&frame.pixels[4..], &[0, 255, 0, 128]);

        assert!(decode_still(&dir.join("nope.png")).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
