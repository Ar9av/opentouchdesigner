//! Application state and the per-frame loop.
//!
//! The loop is: advance time -> collect cook roots -> pull -> draw. Roots are
//! the big viewer plus every node whose viewer is on *and* which is currently
//! on screen. That last condition is what makes the demand-driven engine
//! visible to the user: pan a heavy branch off screen and it stops costing
//! anything (PLAN.md §4).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use egui::TextureId;
use otd_core::{CookContext, CookEngine, Graph, NodeId, OpRegistry, Project};
use otd_gpu::{GpuContext, TopEngine};

use crate::canvas::{CanvasView, DragState};

pub struct OtdApp {
    pub graph: Graph,
    pub registry: OpRegistry,
    pub cook: CookEngine,
    pub top: TopEngine,
    pub render_state: eframe::egui_wgpu::RenderState,

    pub time: CookContext,
    pub playing: bool,
    last_instant: Instant,
    pub smoothed_fps: f64,
    pub smoothed_cook_ms: f64,

    pub selected: Option<NodeId>,
    pub viewer: Option<NodeId>,
    pub view: CanvasView,
    pub drag: Option<DragState>,
    pub create_dialog: Option<CreateDialog>,
    pub show_perf: bool,
    /// A second window showing only the viewer output — the projector feed.
    pub output_window: bool,
    pub output_fullscreen: bool,
    /// Set from inside the output viewport when the user closes it. The
    /// viewport callback is `'static` and cannot reach `self`.
    output_closed: Arc<AtomicBool>,

    thumbs: HashMap<NodeId, (TextureId, u64)>,
    /// Nodes that were on screen last frame — the visible cook roots.
    pub visible: Vec<NodeId>,

    pub project_path: Option<PathBuf>,
    pub status: String,
    pub cook_error: Option<String>,
}

pub struct CreateDialog {
    pub filter: String,
    /// Where the new node lands, in canvas space.
    pub world_pos: egui::Vec2,
    pub focus: bool,
    /// Wire this node's output into the new node's first input, if types allow.
    pub connect_from: Option<NodeId>,
}

impl OtdApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, String> {
        let render_state = cc
            .wgpu_render_state
            .clone()
            .ok_or("OpenTouchDesigner needs the wgpu backend")?;

        // The cook engine renders into the same device egui draws with, so a
        // node thumbnail is the operator's real texture — no copy, no readback.
        let gpu = GpuContext::new(render_state.device.clone(), render_state.queue.clone());
        let top = TopEngine::new(gpu);
        let registry = otd_gpu::ops::registry();

        let mut app = OtdApp {
            graph: Graph::new(),
            registry,
            cook: CookEngine::new(),
            top,
            render_state,
            time: CookContext::default(),
            playing: true,
            last_instant: Instant::now(),
            smoothed_fps: 60.0,
            smoothed_cook_ms: 0.0,
            selected: None,
            viewer: None,
            view: CanvasView::default(),
            drag: None,
            create_dialog: None,
            show_perf: true,
            output_window: false,
            output_fullscreen: false,
            output_closed: Arc::new(AtomicBool::new(false)),
            thumbs: HashMap::new(),
            visible: Vec::new(),
            project_path: None,
            status: String::new(),
            cook_error: None,
        };
        app.load_demo("starter");
        Ok(app)
    }

    /// Load one of the built-in patches (see `otd_gpu::demo`). The editor
    /// opens on `starter`, which is the Phase 0 exit criterion made visible.
    pub fn load_demo(&mut self, name: &str) {
        let Some((graph, out)) = otd_gpu::demo::by_name(name, &self.registry) else {
            self.status = format!("no built-in patch called `{name}`");
            return;
        };
        self.graph = graph;
        self.top.reset();
        self.cook.reset();
        self.thumbs.clear();
        self.project_path = None;
        self.time = CookContext::default();
        self.viewer = Some(out);
        self.selected = self.graph.walk().into_iter().nth(1);
        self.status = format!("{name} patch");
    }

    // -------------------------------------------------------------- cooking

    fn cook_roots(&self) -> Vec<NodeId> {
        let mut roots = Vec::new();
        if let Some(v) = self.viewer.filter(|v| self.graph.contains(*v)) {
            roots.push(v);
        }
        for id in self.graph.walk() {
            if id == self.graph.root() {
                continue;
            }
            let node = self.graph.node(id);
            let wanted = node.flags.render || (node.flags.display && self.visible.contains(&id));
            if wanted && !roots.contains(&id) {
                roots.push(id);
            }
        }
        roots
    }

    fn cook_frame(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last_instant).as_secs_f64().min(0.25);
        self.last_instant = now;
        self.smoothed_fps = self.smoothed_fps * 0.9 + (1.0 / dt.max(1e-6)) * 0.1;

        if self.playing {
            self.time.advance(dt);
        }

        let roots = self.cook_roots();
        self.top.begin_frame();
        let result = self
            .cook
            .cook_frame(&self.graph, &roots, &self.time.clone(), &mut self.top);
        self.top.end_frame();

        self.cook_error = result.err().map(|e| e.to_string());
        let ms = self.cook.stats.total_cook_us as f64 / 1000.0;
        self.smoothed_cook_ms = self.smoothed_cook_ms * 0.9 + ms * 0.1;
    }

    /// The egui texture handle for a node's current output, registering or
    /// refreshing it only when the underlying texture object changed.
    pub fn thumbnail(&mut self, id: NodeId) -> Option<(TextureId, [u32; 2])> {
        let tex = self.top.output(&self.graph, id)?.clone();
        let size = [tex.key.width, tex.key.height];
        let entry = self.thumbs.get(&id).copied();
        match entry {
            Some((tid, seen)) if seen == tex.generation => Some((tid, size)),
            Some((tid, _)) => {
                self.render_state
                    .renderer
                    .write()
                    .update_egui_texture_from_wgpu_texture(
                        &self.render_state.device,
                        &tex.view,
                        wgpu::FilterMode::Linear,
                        tid,
                    );
                self.thumbs.insert(id, (tid, tex.generation));
                Some((tid, size))
            }
            None => {
                let tid = self.render_state.renderer.write().register_native_texture(
                    &self.render_state.device,
                    &tex.view,
                    wgpu::FilterMode::Linear,
                );
                self.thumbs.insert(id, (tid, tex.generation));
                Some((tid, size))
            }
        }
    }

    // ---------------------------------------------------------- graph edits

    pub fn delete_selected(&mut self) {
        let Some(id) = self.selected.take() else {
            return;
        };
        // Rescue the wire: if the node had exactly one input and one consumer,
        // reconnect them so deleting from the middle of a chain isn't
        // destructive.
        let input = self.graph.node(id).inputs.first().copied().flatten();
        let consumers = self.graph.consumers(id);
        if let Some(src) = input {
            for c in &consumers {
                let slots: Vec<usize> = self
                    .graph
                    .node(*c)
                    .inputs
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| **s == Some(id))
                    .map(|(i, _)| i)
                    .collect();
                for slot in slots {
                    let _ = self.graph.connect(src, *c, slot);
                }
            }
        }
        self.top.forget(id);
        self.thumbs.remove(&id);
        self.cook.forget(id);
        let _ = self.graph.remove(id);
        if self.viewer == Some(id) {
            self.viewer = None;
        }
    }

    pub fn create_node(&mut self, type_name: &str, world_pos: egui::Vec2) -> Option<NodeId> {
        let def = self.registry.get(type_name)?.clone();
        let root = self.graph.root();
        let id = self.graph.create(root, &def, None).ok()?;
        self.graph.node_mut_quiet(id).pos = [world_pos.x, world_pos.y];
        self.selected = Some(id);
        Some(id)
    }

    // -------------------------------------------------------- project files

    pub fn save(&mut self, path: Option<PathBuf>) {
        let path = match path.or_else(|| self.project_path.clone()) {
            Some(p) => p,
            None => match rfd::FileDialog::new()
                .add_filter("OpenTouchDesigner project", &["otd"])
                .set_file_name("untitled.otd")
                .save_file()
            {
                Some(p) => p,
                None => return,
            },
        };
        let project = Project::from_graph(&self.graph, &self.registry, self.time.fps);
        match project.save(&path) {
            Ok(()) => {
                self.status = format!("Saved {}", path.display());
                self.project_path = Some(path);
            }
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    pub fn open(&mut self, path: Option<PathBuf>) {
        let path = match path.or_else(|| {
            rfd::FileDialog::new()
                .add_filter("OpenTouchDesigner project", &["otd"])
                .pick_file()
        }) {
            Some(p) => p,
            None => return,
        };
        match Project::load(&path).and_then(|p| p.to_graph(&self.registry)) {
            Ok(graph) => {
                self.graph = graph;
                self.top.reset();
                self.cook.reset();
                self.thumbs.clear();
                self.selected = None;
                // Show the last Null TOP, which is the usual output anchor.
                self.viewer = self
                    .graph
                    .walk()
                    .into_iter()
                    .rfind(|id| self.graph.node(*id).op_type == otd_gpu::ops::NULL);
                self.status = format!("Opened {}", path.display());
                self.project_path = Some(path);
            }
            Err(e) => self.status = format!("Open failed: {e}"),
        }
    }

    pub fn new_project(&mut self) {
        self.load_demo("starter");
    }
}

impl eframe::App for OtdApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Cook before drawing: the thumbnails egui is about to sample are the
        // textures we just rendered into.
        self.cook_frame();

        self.top_bar(ui);
        self.side_panel(ui);
        crate::canvas::show(self, ui);
        self.output_viewport(ui.ctx());

        // A realtime tool repaints continuously; there is always time moving.
        ui.ctx().request_repaint();
    }
}

impl OtdApp {
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("topbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New").clicked() {
                        self.new_project();
                        ui.close();
                    }
                    if ui.button("Open…").clicked() {
                        self.open(None);
                        ui.close();
                    }
                    if ui.button("Save").clicked() {
                        let p = self.project_path.clone();
                        self.save(p);
                        ui.close();
                    }
                    if ui.button("Save As…").clicked() {
                        self.save(None);
                        ui.close();
                    }
                    ui.separator();
                    ui.menu_button("Examples", |ui| {
                        for name in otd_gpu::demo::NAMES {
                            if ui.button(*name).clicked() {
                                self.load_demo(name);
                                ui.close();
                            }
                        }
                    });
                });

                ui.separator();
                let icon = if self.playing { "⏸" } else { "▶" };
                if ui.button(icon).on_hover_text("Play / pause time").clicked() {
                    self.playing = !self.playing;
                }
                if ui.button("⟲").on_hover_text("Reset time to 0").clicked() {
                    self.time.frame = 0;
                    self.time.time = 0.0;
                    self.time.abs_time = 0.0;
                }
                ui.label(format!(
                    "frame {}   {:.2}s",
                    self.time.frame, self.time.abs_time
                ));

                ui.separator();
                ui.toggle_value(&mut self.output_window, "Output")
                    .on_hover_text("Open a second window showing only the viewer");
                if self.output_window {
                    ui.toggle_value(&mut self.output_fullscreen, "Fullscreen");
                }

                ui.separator();
                ui.label(format!("{:.0} fps", self.smoothed_fps));
                ui.label(format!(
                    "cook {:.2} ms  ({} cooked / {} cached)",
                    self.smoothed_cook_ms, self.cook.stats.cooked, self.cook.stats.cached
                ));
                ui.label(format!(
                    "{} passes  {:.0} MB",
                    self.top.passes_this_frame,
                    (self.top.resident_bytes() + self.top.pooled_bytes()) as f64 / 1.0e6
                ));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(err) = &self.cook_error {
                        ui.colored_label(egui::Color32::from_rgb(230, 110, 110), err);
                    } else {
                        ui.label(&self.status);
                    }
                });
            });
        });
    }

    /// The output window: a second OS window showing only the viewer TOP,
    /// which is what gets dragged onto a projector (PLAN.md Phase 1).
    fn output_viewport(&mut self, ctx: &egui::Context) {
        if self.output_closed.swap(false, Ordering::Relaxed) {
            self.output_window = false;
        }
        if !self.output_window {
            return;
        }
        let tex = self
            .viewer
            .filter(|v| self.graph.contains(*v))
            .and_then(|v| self.thumbnail(v));
        let closed = self.output_closed.clone();
        let builder = egui::ViewportBuilder::default()
            .with_title("OpenTouchDesigner — Output")
            .with_inner_size([1280.0, 720.0])
            .with_fullscreen(self.output_fullscreen);

        ctx.show_viewport_deferred(
            egui::ViewportId::from_hash_of("otd-output"),
            builder,
            move |ui, _class| {
                egui::CentralPanel::no_frame()
                    .frame(egui::Frame::NONE.fill(egui::Color32::BLACK))
                    .show(ui, |ui| {
                        if let Some((tid, size)) = tex {
                            // Letterbox rather than stretch: a show output
                            // with the wrong aspect ratio is worse than bars.
                            let avail = ui.available_size();
                            let aspect = size[0] as f32 / size[1].max(1) as f32;
                            let (mut w, mut h) = (avail.x, avail.x / aspect);
                            if h > avail.y {
                                h = avail.y;
                                w = h * aspect;
                            }
                            let rect = egui::Rect::from_center_size(
                                ui.max_rect().center(),
                                egui::vec2(w, h),
                            );
                            ui.painter().image(
                                tid,
                                rect,
                                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                                egui::Color32::WHITE,
                            );
                        }
                    });
                if ui.ctx().input(|i| i.viewport().close_requested()) {
                    closed.store(true, Ordering::Relaxed);
                }
            },
        );
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("params")
            .default_size(360.0)
            .resizable(true)
            .show(ui, |ui| {
                // The output viewer, above the parameters.
                if let Some(v) = self.viewer.filter(|v| self.graph.contains(*v)) {
                    let title = self.graph.path(v);
                    ui.horizontal(|ui| {
                        ui.strong("Viewer");
                        ui.label(egui::RichText::new(title).weak());
                    });
                    if let Some((tid, size)) = self.thumbnail(v) {
                        let avail = ui.available_width();
                        let aspect = size[1] as f32 / size[0].max(1) as f32;
                        let rect_size = egui::vec2(avail, avail * aspect);
                        ui.add(
                            egui::Image::new((tid, rect_size))
                                .fit_to_exact_size(rect_size)
                                .corner_radius(4.0),
                        );
                        ui.label(
                            egui::RichText::new(format!("{} × {}", size[0], size[1]))
                                .weak()
                                .small(),
                        );
                    }
                    ui.separator();
                }

                egui::ScrollArea::vertical().show(ui, |ui| {
                    crate::params::show(self, ui);
                });
            });
    }
}
