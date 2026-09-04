//! Running a project without a window.
//!
//! The editor's frame loop is: advance time, collect cook roots, pull, draw.
//! Everything except the drawing is already headless — `TopEngine` renders to
//! its own textures and only hands one to a surface at the very end — so this
//! is the same loop with the last step removed. That is deliberate: a show
//! machine and a render node should be running the *same* engine as the
//! editor, not a reimplementation that drifts.

use std::path::Path;
use std::time::Instant;

use otd_core::{CookContext, CookEngine, Graph, NodeId, OpRegistry, Project};
use otd_engine::Engines;
use otd_gpu::GpuContext;

pub struct Runtime {
    pub graph: Graph,
    pub registry: OpRegistry,
    pub engines: Engines,
    pub cook: CookEngine,
    pub time: CookContext,
    /// The GPU context, kept for pixel readback.
    pub gpu: GpuContext,
}

/// What one frame cost, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameTiming {
    pub cook_ms: f64,
    /// Wall-clock for the whole frame, which includes GPU submission and any
    /// readback. The number that decides whether a show holds its rate.
    pub wall_ms: f64,
    pub cooked: u32,
    pub cached: u32,
}

impl Runtime {
    pub fn open(path: &Path) -> Result<Runtime, String> {
        let registry = otd_engine::registry();
        let project = Project::load(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let fps = project.fps;
        let graph = project
            .to_graph(&registry)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let gpu = GpuContext::headless().map_err(|e| format!("no GPU: {e}"))?;

        let time = CookContext {
            fps,
            ..Default::default()
        };
        Ok(Runtime {
            engines: Engines::new(gpu.clone()),
            cook: CookEngine::new(),
            graph,
            registry,
            time,
            gpu,
        })
    }

    /// The nodes worth cooking when nobody is looking at a viewer.
    ///
    /// In the editor, what you can *see* is a cook root: the viewer, and any
    /// node whose display flag is on and which is currently on screen. None of
    /// that exists here, and the display flag is on by default, so using it
    /// would mean cooking every node in the project including the branches
    /// nobody wired to anything.
    ///
    /// So headless the roots are the render flag alone — the one flag that
    /// already means "cook this whether or not anything downstream wants it".
    pub fn roots(&self) -> Vec<NodeId> {
        self.graph
            .walk()
            .into_iter()
            .filter(|id| *id != self.graph.root() && self.graph.node(*id).flags.render)
            .collect()
    }

    /// Resolve a node path like `/out1`, for `--node`.
    pub fn find(&self, path: &str) -> Result<NodeId, String> {
        self.graph
            .find(path)
            .ok_or_else(|| format!("no node at `{path}`"))
    }

    /// Cook one frame. `dt` is the time step to advance by first.
    pub fn frame(&mut self, roots: &[NodeId], dt: f64) -> Result<FrameTiming, String> {
        let started = Instant::now();
        self.time.advance(dt);
        self.graph.sync_clones(&self.registry);

        self.engines.begin_frame();
        let result = self
            .cook
            .cook_frame(&self.graph, roots, &self.time.clone(), &mut self.engines);
        self.engines.end_frame();
        result.map_err(|e| e.to_string())?;

        Ok(FrameTiming {
            cook_ms: self.cook.stats.total_cook_us as f64 / 1000.0,
            wall_ms: started.elapsed().as_secs_f64() * 1000.0,
            cooked: self.cook.stats.cooked,
            cached: self.cook.stats.cached,
        })
    }

    /// Read a TOP's current output as 8-bit RGBA.
    pub fn read(&self, id: NodeId) -> Result<(u32, u32, Vec<u8>), String> {
        let tex = self
            .engines
            .top
            .output(&self.graph, id)
            .ok_or_else(|| format!("{} produced no texture", self.graph.path(id)))?;
        otd_gpu::read_pixels_rgba8(&self.gpu, tex)
    }

    /// Any shader that failed to compile, as `path: message`.
    ///
    /// Worth reporting loudly: a broken shader holds its last good pipeline,
    /// so headless it would otherwise show up as a sequence of frames that
    /// look plausible and are wrong.
    pub fn shader_errors(&self) -> Vec<String> {
        self.graph
            .walk()
            .into_iter()
            .filter_map(|id| {
                self.engines
                    .top
                    .shader_error(id)
                    .map(|e| format!("{}: {e}", self.graph.path(id)))
            })
            .collect()
    }
}

/// The median and the worst of a set of frame times.
///
/// Mean frame time is the wrong summary for a realtime system: one 40 ms frame
/// among sixty good ones is a visible hitch and barely moves the mean. The
/// median says what it usually does and the maximum says what went wrong.
pub fn summarize(mut values: Vec<f64>) -> (f64, f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = values[values.len() / 2];
    let p95 = values[(values.len() * 95 / 100).min(values.len() - 1)];
    let max = *values.last().unwrap();
    (median, p95, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_reports_the_typical_frame_and_the_worst_one() {
        // Fifty good frames and one hitch. The mean would be 2.7 ms and would
        // say nothing; the maximum is the whole story.
        let mut v = vec![2.0; 50];
        v.push(40.0);
        let (median, _, max) = summarize(v);
        assert_eq!(median, 2.0);
        assert_eq!(max, 40.0);
    }

    #[test]
    fn summarize_survives_an_empty_run() {
        assert_eq!(summarize(vec![]), (0.0, 0.0, 0.0));
    }
}
