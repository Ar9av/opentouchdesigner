//! Watch: turn the camera on, show the assistant what it sees, build an
//! effect for it, and look at what was built.
//!
//! The assistant has always been able to take a picture — a reference to
//! work back from. What it could not do was look at the *input*: asked to
//! "make my camera cool" it was designing blind, for a face it had not seen
//! in a room it did not know was dark. And it never saw what it built, so a
//! patch that applied cleanly and showed black was reported as done.
//!
//! Four stages, one per frame tick, no threads of their own:
//!
//!  1. **Camera** — find the camera on the canvas or add one, and wait for a
//!     frame that is not black (the permission prompt, the device warming
//!     up). Bounded, and says why when it gives up.
//!  2. **Asked** — the frame goes out with [`otd_ai::Seen::Camera`], whose
//!     brief says "this is the input, design for it".
//!  3. **Settling** — once the plan lands, let it run, then read the viewer
//!     and the camera twice, half a second apart, and measure them with the
//!     same ruler the eval harness uses ([`otd_ai::vision::Measured`]).
//!  4. **One repair** — if the result is black, blown out, frozen, the
//!     camera unchanged or so faint nobody would notice, the viewer frame goes back with
//!     [`otd_ai::Seen::Result`] naming exactly that. Once: a second failure is
//!     reported, not looped on, because every round is a paid request.
//!
//! The measuring is local, so a result that looks alive costs nothing extra.

use std::time::{Duration, Instant};

use otd_ai::Seen;
use otd_ai::vision::{Measured, judging_size, mean_luma};
use otd_core::NodeId;

use crate::app::OtdApp;

/// How long a camera gets to deliver its first real frame. The macOS
/// permission prompt sits inside this, so it is generous.
const CAMERA_PATIENCE: Duration = Duration::from_secs(20);
/// Readbacks stall the pipeline; a few a second is plenty for "is it on yet".
const POLL: Duration = Duration::from_millis(250);
/// How long a new patch runs before it is judged — long enough for a
/// feedback loop to build up, short enough to still feel like one action.
const SETTLE: Duration = Duration::from_millis(1500);
/// The gap between the two readings that decide whether it moves.
const GAP: Duration = Duration::from_millis(500);

pub enum Watch {
    Camera { camera: NodeId, since: Instant, polled: Instant },
    Asked { camera: NodeId, repaired: bool },
    Settling { camera: NodeId, repaired: bool, since: Instant, early: Option<(Vec<u8>, Vec<u8>)> },
}

impl Watch {
    /// One line for the bar while it is going on.
    pub fn label(&self) -> &'static str {
        match self {
            Watch::Camera { .. } => "waiting for the camera…",
            Watch::Asked { repaired: false, .. } => "looking at the camera…",
            Watch::Asked { repaired: true, .. } => "fixing what it saw…",
            Watch::Settling { .. } => "checking the result…",
        }
    }
}

/// Start watching, or stop if already watching.
pub fn toggle(app: &mut OtdApp) {
    if app.assistant.watch.take().is_some() {
        app.assistant.status = "stopped watching".into();
        return;
    }
    app.assistant.error = None;
    let camera = find_camera(app).or_else(|| {
        crate::media::add_webcam(app);
        find_camera(app)
    });
    let Some(camera) = camera else {
        app.assistant.error = Some("could not add a camera to this network".into());
        return;
    };
    let now = Instant::now();
    app.assistant.watch = Some(Watch::Camera { camera, since: now, polled: now - POLL });
}

/// The selected camera, or the first one in the network being looked at.
fn find_camera(app: &OtdApp) -> Option<NodeId> {
    let is_camera = |id: &NodeId| app.graph.node(*id).op_type == otd_gpu::ops::VIDEO_DEVICE_IN;
    app.selected
        .filter(|id| app.graph.contains(*id))
        .filter(is_camera)
        .or_else(|| app.graph.children(app.current).iter().copied().find(is_camera))
}

/// A node's current output as RGBA8. Stalls the GPU, so called sparingly.
fn read(app: &OtdApp, id: NodeId) -> Option<(u32, u32, Vec<u8>)> {
    let tex = app.engines.top.output(&app.graph, id)?.clone();
    let gpu = otd_gpu::GpuContext::new(app.render_state.device.clone(), app.render_state.queue.clone());
    otd_gpu::read_pixels_rgba8(&gpu, &tex).ok()
}

fn read_small(app: &OtdApp, id: NodeId) -> Option<Vec<u8>> {
    let (w, h, rgba) = read(app, id)?;
    judging_size(w, h, rgba)
}

/// Send a frame with the given brief, keeping whatever the user typed as a
/// refinement on top of it.
fn ask_about(app: &mut OtdApp, frame: (u32, u32, Vec<u8>), seen: Seen) -> Result<(), String> {
    let (w, h, rgba) = frame;
    app.assistant.image = Some(otd_ai::Image::from_rgba(w, h, rgba)?);
    app.assistant.seen = seen;
    crate::assistant::send(app);
    // The frame was for this request only; the next thing typed is not about
    // a picture taken a minute ago.
    app.assistant.detach();
    app.assistant.prompt.clear();
    match &app.assistant.error {
        Some(e) => Err(e.clone()),
        None => Ok(()),
    }
}

/// Advance the watch by one frame. Cheap when there is nothing to do.
pub fn tick(app: &mut OtdApp) {
    let Some(state) = app.assistant.watch.take() else {
        return;
    };
    app.assistant.watch = step(app, state);
}

fn step(app: &mut OtdApp, state: Watch) -> Option<Watch> {
    let now = Instant::now();
    match state {
        Watch::Camera { camera, since, polled } => {
            if !app.graph.contains(camera) {
                app.assistant.error = Some("the camera was removed".into());
                return None;
            }
            if now - polled < POLL {
                return Some(Watch::Camera { camera, since, polled });
            }
            let frame = read(app, camera).filter(|(_, _, p)| mean_luma(p) > 2.0);
            match frame {
                Some(frame) => match ask_about(app, frame, Seen::Camera) {
                    Ok(()) => Some(Watch::Asked { camera, repaired: false }),
                    Err(_) => None,
                },
                None if now - since > CAMERA_PATIENCE => {
                    app.assistant.error = Some(
                        "the camera never showed a picture — check System Settings › \
                         Privacy › Camera, or that no other app is holding it"
                            .into(),
                    );
                    None
                }
                None => Some(Watch::Camera { camera, since, polled: now }),
            }
        }
        Watch::Asked { camera, repaired } => {
            if app.assistant.busy() {
                return Some(Watch::Asked { camera, repaired });
            }
            // An error is already on the bar; a landed plan leaves notes.
            if app.assistant.error.is_some() || app.assistant.last.is_none() {
                return None;
            }
            Some(Watch::Settling { camera, repaired, since: now, early: None })
        }
        Watch::Settling { camera, repaired, since, early } => {
            let viewer = app.viewer.filter(|id| app.graph.contains(*id))?;
            let elapsed = now - since;
            if elapsed < SETTLE {
                return Some(Watch::Settling { camera, repaired, since, early });
            }
            let Some(early) = early else {
                let pair = read_small(app, viewer).zip(read_small(app, camera));
                return pair.map(|pair| Watch::Settling { camera, repaired, since, early: Some(pair) });
            };
            if elapsed < SETTLE + GAP {
                return Some(Watch::Settling { camera, repaired, since, early: Some(early) });
            }
            let (Some(late), Some(src_late)) = (read_small(app, viewer), read_small(app, camera)) else {
                return None;
            };
            let measured = Measured::of(&early.0, &late, &early.1, &src_late);
            let mut faults = measured.faults();
            // Watch was asked to make it look good, not merely to work.
            if faults.is_empty() && measured.is_subtle() {
                faults.push("SUBTLE");
            }
            if faults.is_empty() {
                app.assistant.status = "looked at the result: alive, and not the raw camera".into();
                return None;
            }
            if repaired {
                app.assistant.warnings.push(format!(
                    "still {} after one fix — Cmd/Ctrl+Z, or say what to change",
                    faults.join(", ")
                ));
                return None;
            }
            let frame = read(app, viewer)?;
            match ask_about(app, frame, Seen::Result(faults)) {
                Ok(()) => Some(Watch::Asked { camera, repaired: true }),
                Err(_) => None,
            }
        }
    }
}
