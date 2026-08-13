//! Application state and the per-frame loop.
//!
//! The loop is: advance time -> collect cook roots -> pull -> draw. Roots are
//! the big viewer plus every node whose viewer is on *and* which is currently
//! on screen. That last condition is what makes the demand-driven engine
//! visible to the user: pan a heavy branch off screen and it stops costing
//! anything (PLAN.md §4).

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use egui::TextureId;
use otd_core::{CookContext, CookEngine, Graph, History, NodeId, OpRegistry, Project};
use otd_engine::Engines;
use otd_gpu::GpuContext;

use crate::canvas::{CanvasView, DragState};

pub struct OtdApp {
    pub graph: Graph,
    pub registry: OpRegistry,
    pub cook: CookEngine,
    pub engines: Engines,
    pub render_state: eframe::egui_wgpu::RenderState,

    pub time: CookContext,
    pub playing: bool,
    /// The looping range, in seconds. Time wraps to the start when it passes
    /// the end, which is what makes a timeline a *loop* rather than a stopwatch.
    pub loop_range: (f64, f64),
    pub looping: bool,
    /// Set while the playhead is being dragged, so the transport does not
    /// fight the scrub.
    scrubbing: bool,
    last_instant: Instant,
    pub smoothed_fps: f64,
    pub smoothed_cook_ms: f64,

    /// The node the parameter panel is showing, and the referent of "it" in
    /// an assistant request. The *primary* of the selection rather than a
    /// second idea of what is selected: whenever `selection` is non-empty this
    /// is one of its members, and clearing one clears the other. Keeping it
    /// separate is what lets a five-node selection still have a parameter
    /// panel — the alternative is showing nothing the moment you select two
    /// things, which is worse.
    pub selected: Option<NodeId>,
    /// Everything selected, primary included. Delete and drag act on all of
    /// it; the parameter panel acts on `selected`.
    pub selection: BTreeSet<NodeId>,
    pub viewer: Option<NodeId>,
    /// The component whose network is on screen. Entering a component is
    /// just changing this — the graph is one tree throughout.
    pub current: NodeId,
    pub view: CanvasView,
    /// Where the network editor was drawn last frame. Drag-and-drop needs it
    /// to turn a pointer position into a place in the network.
    pub canvas_rect: egui::Rect,
    pub drag: Option<DragState>,
    pub create_dialog: Option<CreateDialog>,
    pub show_perf: bool,
    /// The performance monitor window.
    pub show_monitor: bool,
    /// Perform mode: the editor is gone and the main window shows only the
    /// output. F1 in, F1 or Escape out.
    pub perform: bool,
    /// A second window showing only the viewer output — the projector feed.
    pub output_window: bool,
    pub output_fullscreen: bool,
    /// Set from inside the output viewport when the user closes it. The
    /// viewport callback is `'static` and cannot reach `self`.
    output_closed: Arc<AtomicBool>,

    /// Mouse and keyboard, sampled each frame for the input CHOPs.
    pub input_state: otd_chop::InputState,
    /// Draft state for the "add a component parameter" row.
    pub new_param_name: String,
    pub new_param_type: String,

    thumbs: HashMap<NodeId, (TextureId, u64)>,
    /// Nodes that were on screen last frame — the visible cook roots.
    pub visible: Vec<NodeId>,

    pub history: History,

    /// The assistant panel: describe a patch, get operators.
    pub assistant: crate::assistant::Assistant,

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
        let engines = Engines::new(gpu);
        let registry = otd_engine::registry();

        let mut app = OtdApp {
            graph: Graph::new(),
            registry,
            cook: CookEngine::new(),
            engines,
            render_state,
            time: CookContext::default(),
            playing: true,
            loop_range: (0.0, 10.0),
            looping: false,
            scrubbing: false,
            last_instant: Instant::now(),
            smoothed_fps: 60.0,
            smoothed_cook_ms: 0.0,
            selected: None,
            selection: BTreeSet::new(),
            viewer: None,
            current: NodeId::default(),
            view: CanvasView::default(),
            canvas_rect: egui::Rect::NOTHING,
            drag: None,
            create_dialog: None,
            show_perf: true,
            show_monitor: false,
            perform: false,
            output_window: false,
            output_fullscreen: false,
            output_closed: Arc::new(AtomicBool::new(false)),
            input_state: otd_chop::InputState::default(),
            new_param_name: String::new(),
            new_param_type: "float".to_string(),
            thumbs: HashMap::new(),
            visible: Vec::new(),
            history: History::default(),
            assistant: crate::assistant::Assistant::default(),
            project_path: None,
            status: String::new(),
            cook_error: None,
        };
        // Not `starter`: that patch is the Phase 0 exit criterion, and an
        // animated grey noise field is a bad first argument for what this
        // tool is. `tunnel` is nine nodes, no shader, and looks like the
        // thing somebody opened the program hoping to make.
        app.load_demo("tunnel");
        Ok(app)
    }

    /// Load one of the built-in patches (see `otd_gpu::demo`). The editor
    /// opens on `starter`, which is the Phase 0 exit criterion made visible.
    pub fn load_demo(&mut self, name: &str) {
        let Some((graph, out)) = otd_engine::demo::by_name(name, &self.registry) else {
            self.status = format!("no built-in patch called `{name}`");
            return;
        };
        self.graph = graph;
        self.engines.reset();
        self.cook.reset();
        self.thumbs.clear();
        self.history.clear();
        self.project_path = None;
        self.time = CookContext::default();
        self.current = self.graph.root();
        self.viewer = Some(out);
        self.clear_selection();
        if let Some(first) = self.graph.walk().into_iter().nth(1) {
            self.select_only(first);
        }
        self.status = format!("{name} patch");
    }

    // -------------------------------------------------------------- cooking

    fn cook_roots(&self) -> Vec<NodeId> {
        cook_roots(&self.graph, self.viewer, &self.visible)
    }

    fn cook_frame(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last_instant).as_secs_f64().min(0.25);
        self.last_instant = now;
        self.smoothed_fps = self.smoothed_fps * 0.9 + (1.0 / dt.max(1e-6)) * 0.1;

        if self.playing && !self.scrubbing {
            self.time.advance(dt);
            let (start, end) = self.loop_range;
            if self.looping && end > start && self.time.time >= end {
                // Wrap by the loop length rather than snapping to the start:
                // the leftover carries into the next pass, so a loop shorter
                // than a frame interval does not drift or stall.
                let length = end - start;
                self.time.time = start + (self.time.time - start).rem_euclid(length);
            }
        }
        self.scrubbing = false;

        // Clones follow their master. An unchanged master costs one subtree
        // walk, so this is cheap enough to do every frame and means the
        // editor never shows a stale copy.
        // Replicators first — they may create clones for syncing to fill.
        let replicated = otd_engine::replicator::sync(&mut self.graph, &self.engines.dats);
        let synced = self.graph.sync_clones(&self.registry);
        if replicated > 0 {
            self.status = format!("replicated {replicated} node(s)");
        } else if synced > 0 {
            self.status = format!("synced {synced} clone(s)");
        }

        let roots = self.cook_roots();
        self.engines.set_input_state(self.input_state.clone());
        self.engines.begin_frame();
        let result =
            self.cook
                .cook_frame(&self.graph, &roots, &self.time.clone(), &mut self.engines);
        self.engines.end_frame();

        self.cook_error = result.err().map(|e| e.to_string());

        // Between frames, where the graph is held mutably, is the only place a
        // callback's parameter change can land. See `otd_core::edit`.
        let edits = self.engines.take_edits();
        if !edits.is_empty() {
            let (applied, problems) = otd_core::edit::apply(&mut self.graph, &edits);
            if let Some(first) = problems.first() {
                self.status = format!("callback: {first}");
            } else if applied > 0 {
                self.status = format!("callback set {applied} parameter(s)");
            }
        }
        let ms = self.cook.stats.total_cook_us as f64 / 1000.0;
        self.smoothed_cook_ms = self.smoothed_cook_ms * 0.9 + ms * 0.1;
    }

    /// The egui texture handle for a node's current output, registering or
    /// refreshing it only when the underlying texture object changed.
    pub fn thumbnail(&mut self, id: NodeId) -> Option<(TextureId, [u32; 2])> {
        let tex = self.engines.top.output(&self.graph, id)?.clone();
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

    /// Write the selected component out as a `.otdc`.
    pub fn save_component(&mut self) {
        let Some(id) = self.selected.filter(|s| self.graph.contains(*s)) else {
            return;
        };
        if self.graph.node(id).family != otd_core::Family::Comp {
            self.status = "select a component first".into();
            return;
        }
        let name = self.graph.node(id).name.clone();
        let Some(path) = rfd::FileDialog::new()
            .add_filter("OpenTouchDesigner component", &["otdc"])
            .set_file_name(format!("{name}.otdc"))
            .save_file()
        else {
            return;
        };
        match otd_core::Component::from_graph(&self.graph, id, &self.registry) {
            Some(c) => match c.save(&path) {
                Ok(()) => self.status = format!("Saved component {}", path.display()),
                Err(e) => self.status = format!("Save failed: {e}"),
            },
            None => self.status = "could not read that component".into(),
        }
    }

    /// Bring a `.otdc` into the current network, linked to its file so later
    /// edits to the shared definition arrive here.
    pub fn import_component(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("OpenTouchDesigner component", &["otdc"])
            .pick_file()
        else {
            return;
        };
        self.import_component_at(&path);
    }

    /// The same, for a `.otdc` that arrived by being dropped on the network.
    pub fn import_component_at(&mut self, path: &std::path::Path) -> Option<NodeId> {
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "component".into());
        let def = self.registry.get("containerCOMP").cloned()?;
        let parent = self.current;
        self.edit("import component");
        self.history.end_gesture();
        let id = self.graph.create(parent, &def, Some(&name)).ok()?;
        match self
            .graph
            .attach_external(id, &path.to_string_lossy(), &self.registry)
        {
            Ok(()) => {
                self.select_only(id);
                self.status = format!("Imported {}", path.display());
                Some(id)
            }
            Err(e) => {
                let _ = self.graph.remove(id);
                self.status = format!("Import failed: {e}");
                None
            }
        }
    }

    /// Copy the project and everything it references into one folder.
    ///
    /// A project on an authoring machine points at shared `.otdc` files that
    /// live wherever the artist keeps them. A show machine has none of those
    /// directories. Exporting copies them in and rewrites the references to be
    /// relative, so the folder can be moved anywhere and still open.
    pub fn export_bundle(&mut self) {
        let name = self
            .project_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "show".into());
        let Some(dir) = rfd::FileDialog::new()
            .set_file_name(&name)
            .set_title("Export bundle into a new folder")
            .pick_folder()
        else {
            return;
        };
        match otd_core::bundle::export(&self.graph, &self.registry, self.time.fps, &dir, &name) {
            Ok(bundle) => {
                self.status = format!(
                    "Exported {} — {} component(s), {} media file(s){}",
                    bundle.project.display(),
                    bundle.components.len(),
                    bundle.media.len(),
                    if bundle.missing.is_empty() {
                        String::new()
                    } else {
                        // Loud, not silent: a bundle missing a component will
                        // fail on the show machine, not here.
                        format!(
                            ", {} MISSING: {}",
                            bundle.missing.len(),
                            bundle
                                .missing
                                .iter()
                                .map(|(path, file, _)| format!("{path} -> {file}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    }
                );
            }
            Err(e) => self.status = format!("Export failed: {e}"),
        }
    }

    /// Load an `.fs` ISF shader onto a GLSL TOP.
    ///
    /// The point of ISF is that thousands of effects already exist in it. This
    /// turns one of them into an ordinary node with ordinary parameters, so it
    /// can be bound, exported to and saved like anything else.
    pub fn import_isf(&mut self, id: NodeId) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("ISF shader", &["fs", "isf", "frag"])
            .pick_file()
        else {
            return;
        };
        let source = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                self.status = format!("Import failed: {e}");
                return;
            }
        };
        match otd_gpu::isf::import(&source) {
            Ok(isf) => {
                self.edit("import ISF");
                self.history.end_gesture();
                let count = isf.params.len();
                otd_gpu::isf::apply(&mut self.graph, id, &isf);
                let name = path.file_stem().unwrap_or_default().to_string_lossy();
                self.status = format!("Imported {name} — {count} parameter(s)");
            }
            Err(e) => self.status = format!("Import failed: {e}"),
        }
    }

    // --------------------------------------------------------- selection
    //
    // Every path in and out of the selection goes through these, so the
    // invariant — `selected` is in `selection`, or both are empty — holds by
    // construction rather than by everybody remembering.

    /// Click with no modifier: this node and nothing else.
    pub fn select_only(&mut self, id: NodeId) {
        self.selection.clear();
        self.selection.insert(id);
        self.selected = Some(id);
    }

    /// Shift- or Cmd-click: add if absent, remove if present.
    pub fn select_toggle(&mut self, id: NodeId) {
        if self.selection.remove(&id) {
            // Removing the primary promotes whatever is left, so a selection
            // that still has members still has a parameter panel.
            if self.selected == Some(id) {
                self.selected = self.selection.iter().next().copied();
            }
        } else {
            self.selection.insert(id);
            self.selected = Some(id);
        }
    }

    /// Cmd/Ctrl+A: everything in the network on screen.
    ///
    /// The network on screen, not the whole graph — the canvas is the thing
    /// being selected into, and reaching inside components you are not
    /// looking at would make the next Delete much bigger than it looked.
    pub fn select_all(&mut self) {
        self.selection = self.graph.node(self.current).children.iter().copied().collect();
        // Keep the primary if it survived, so Cmd+A does not swap the
        // parameter panel out from under you.
        if !self.selected.map(|s| self.selection.contains(&s)).unwrap_or(false) {
            self.selected = self.selection.iter().next().copied();
        }
    }

    pub fn clear_selection(&mut self) {
        self.selection.clear();
        self.selected = None;
    }

    pub fn is_selected(&self, id: NodeId) -> bool {
        self.selection.contains(&id)
    }

    /// Cmd/Ctrl+A, and Escape to drop the selection.
    ///
    /// Behind the text-focus guard: Cmd+A in a parameter field is select-all
    /// *text*, and stealing it would make the fields hard to edit.
    fn selection_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::A)) {
            self.select_all();
        }
    }

    // -------------------------------------------------------- navigation

    /// Step inside a component. Anything else is ignored, so a stray
    /// double-click on an operator does not lose your place.
    pub fn enter(&mut self, id: NodeId) {
        if self.graph.get(id).map(|n| n.family) == Some(otd_core::Family::Comp) {
            self.current = id;
            self.clear_selection();
            self.view = CanvasView::default();
        }
    }

    /// Step out to the parent network.
    pub fn leave(&mut self) {
        if let Some(parent) = self.graph.get(self.current).and_then(|n| n.parent) {
            let was = self.current;
            self.current = parent;
            self.select_only(was);
            self.view = CanvasView::default();
        }
    }

    // -------------------------------------------------------------- history

    /// Cmd/Ctrl+Z and Cmd/Ctrl+Shift+Z (or Cmd+Y).
    ///
    /// Handled here rather than on the canvas so undo works with the parameter
    /// panel focused and in perform mode too. While a text field has focus,
    /// egui's own character-level undo is the right behaviour and wins.
    fn history_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (undo, redo) = ctx.input(|i| {
            let cmd = i.modifiers.command;
            (
                cmd && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                cmd && ((i.modifiers.shift && i.key_pressed(egui::Key::Z))
                    || i.key_pressed(egui::Key::Y)),
            )
        });
        if redo {
            self.redo();
        } else if undo {
            self.undo();
        }
    }

    /// Cmd/Ctrl+S, and Cmd/Ctrl+Shift+S for Save As.
    ///
    /// Deliberately *not* behind the "is a text field focused" guard that
    /// history uses. Undo defers to egui's character-level undo while you are
    /// typing, which is right; saving has no such conflict, and the moment
    /// somebody most wants Cmd+S is right after typing a name into a
    /// parameter.
    fn file_keys(&mut self, ctx: &egui::Context) {
        let (save, save_as) = ctx.input(|i| {
            let cmd = i.modifiers.command;
            (
                cmd && !i.modifiers.shift && i.key_pressed(egui::Key::S),
                cmd && i.modifiers.shift && i.key_pressed(egui::Key::S),
            )
        });
        if save_as {
            self.save(None);
        } else if save {
            // `save` falls back to asking for a path when the project has
            // never been written, so this is Save As for an untitled patch
            // without being a second shortcut.
            let path = self.project_path.clone();
            self.save(path);
        }
    }

    /// Record the graph before a change, so it can be undone.
    ///
    /// `tag` names what is being edited. While it stays the same the edits are
    /// one entry, which is what makes dragging a slider undo as the gesture it
    /// was rather than as the sixty frames it took.
    pub fn edit(&mut self, tag: &str) {
        self.history.checkpoint(&self.graph, tag);
    }

    pub fn undo(&mut self) {
        match self.history.undo(&self.graph) {
            Some(graph) => {
                self.restore(graph);
                self.status = "Undo".into();
            }
            None => self.status = "nothing to undo".into(),
        }
    }

    pub fn redo(&mut self) {
        match self.history.redo(&self.graph) {
            Some(graph) => {
                self.restore(graph);
                self.status = "Redo".into();
            }
            None => self.status = "nothing to redo".into(),
        }
    }

    /// Swap in a restored graph and repair everything that pointed into the
    /// old one.
    ///
    /// Node ids survive a snapshot, so the selection and the viewer usually
    /// still resolve — but not if the step being undone was a delete, or the
    /// step being redone was a create. Those are exactly the cases that would
    /// otherwise leave a dangling id in the cook roots.
    fn restore(&mut self, graph: Graph) {
        self.graph = graph;
        self.selected = self.selected.filter(|id| self.graph.contains(*id));
        self.selection.retain(|id| self.graph.contains(*id));
        if self.selected.is_none() {
            self.selected = self.selection.iter().next().copied();
        }
        self.viewer = self.viewer.filter(|id| self.graph.contains(*id));
        if !self.graph.contains(self.current) {
            self.current = self.graph.root();
        }
        self.visible.retain(|id| self.graph.contains(*id));
        self.drag = None;
    }

    // ---------------------------------------------------------- graph edits

    /// Delete everything selected, as one undo step.
    pub fn delete_selected(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.edit("delete");
        self.history.end_gesture();
        let doomed: Vec<NodeId> = std::mem::take(&mut self.selection).into_iter().collect();
        self.selected = None;
        self.delete_all(&doomed);
    }

    /// Remove a set of nodes as one gesture, healing the chains they sat in.
    ///
    /// The splices are all worked out first, against the graph as it stands.
    /// Healing node by node reads a graph the previous removal already
    /// changed: deleting `b` and `c` from `a -> b -> c -> d` would look up
    /// `c`'s input after `b` had gone, find nothing, and leave `d` empty —
    /// which is a black texture and looks like the delete broke the patch.
    fn delete_all(&mut self, doomed: &[NodeId]) {
        let live: Vec<NodeId> = doomed
            .iter()
            .copied()
            .filter(|id| self.graph.contains(*id))
            .collect();
        let splices = otd_ai::patch::splices_for(&self.graph, &live);
        for id in &live {
            self.engines.top.forget(*id);
            self.thumbs.remove(id);
            self.cook.forget(*id);
            let _ = self.graph.remove(*id);
            if self.viewer == Some(*id) {
                self.viewer = None;
            }
        }
        for (src, consumer, slot) in splices {
            let _ = self.graph.connect(src, consumer, slot);
        }
    }

    /// Empty the network on screen — the `/clear` command and the File menu.
    ///
    /// One undo, like everything else the assistant does, because the whole
    /// point of it is to be the fast way back from a patch that went wrong.
    pub fn clear_network(&mut self) -> usize {
        let doomed: Vec<NodeId> = self.graph.node(self.current).children.clone();
        if doomed.is_empty() {
            return 0;
        }
        self.edit("clear");
        self.history.end_gesture();
        self.clear_selection();
        self.delete_all(&doomed);
        self.viewer = None;
        doomed.len()
    }

    pub fn create_node(&mut self, type_name: &str, world_pos: egui::Vec2) -> Option<NodeId> {
        let def = self.registry.get(type_name)?.clone();
        self.edit("create");
        self.history.end_gesture();
        // New operators land in the network you are looking at.
        let parent = self.current;
        let id = self.graph.create(parent, &def, None).ok()?;
        self.graph.node_mut_quiet(id).pos = [world_pos.x, world_pos.y];
        self.select_only(id);
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
        match Project::open(&path, &self.registry) {
            Ok(graph) => {
                self.graph = graph;
                self.engines.top.reset();
                self.cook.reset();
                self.thumbs.clear();
                // Undoing across an Open would restore the previous project's
                // graph into this project's file. History starts here.
                self.history.clear();
                self.clear_selection();
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

        self.history_keys(ui.ctx());
        self.file_keys(ui.ctx());
        self.selection_keys(ui.ctx());

        // F1 anywhere, including out of perform mode — a performer who cannot
        // find the way back out of a black screen has a real problem.
        if ui.ctx().input(|i| i.key_pressed(egui::Key::F1)) {
            self.perform = !self.perform;
        }
        if self.perform && ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
            self.perform = false;
        }

        if self.perform {
            self.perform_view(ui);
        } else {
            self.top_bar(ui);
            self.monitor(ui.ctx());
            self.timeline(ui);
            self.side_panel(ui);
            crate::canvas::show(self, ui);
        }
        crate::assistant::bar(self, ui.ctx());
        crate::assistant::window(self, ui.ctx());
        self.output_viewport(ui.ctx());

        // Files can be dropped in either mode: a performer swapping a clip
        // mid-show should not have to leave the output to do it.
        crate::media::hover_overlay(self, ui.ctx());
        crate::media::handle_drops(self, ui.ctx());

        // A realtime tool repaints continuously; there is always time moving.
        ui.ctx().request_repaint();
    }
}

impl OtdApp {
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("topbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("Edit", |ui| {
                    let (back, ahead) = self.history.depth();
                    if ui
                        .add_enabled(back > 0, egui::Button::new("Undo").shortcut_text("Cmd+Z"))
                        .clicked()
                    {
                        self.undo();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            ahead > 0,
                            egui::Button::new("Redo").shortcut_text("Cmd+Shift+Z"),
                        )
                        .clicked()
                    {
                        self.redo();
                        ui.close();
                    }
                });
                ui.menu_button("File", |ui| {
                    if ui.button("New").clicked() {
                        self.new_project();
                        ui.close();
                    }
                    if ui.button("Open…").clicked() {
                        self.open(None);
                        ui.close();
                    }
                    if ui
                        .add(egui::Button::new("Save").shortcut_text("Cmd+S"))
                        .clicked()
                    {
                        let p = self.project_path.clone();
                        self.save(p);
                        ui.close();
                    }
                    if ui
                        .add(egui::Button::new("Save As…").shortcut_text("Cmd+Shift+S"))
                        .clicked()
                    {
                        self.save(None);
                        ui.close();
                    }
                    if ui
                        .button("Export Bundle…")
                        .on_hover_text(
                            "Copy the project and every component it uses into one folder, \
                             ready to move to a show machine",
                        )
                        .clicked()
                    {
                        self.export_bundle();
                        ui.close();
                    }
                    ui.separator();
                    let on_comp = self
                        .selected
                        .and_then(|s| self.graph.get(s))
                        .map(|n| n.family == otd_core::Family::Comp)
                        .unwrap_or(false);
                    if ui
                        .add_enabled(on_comp, egui::Button::new("Save Component As…"))
                        .on_hover_text("Write the selected component to a .otdc file")
                        .clicked()
                    {
                        self.save_component();
                        ui.close();
                    }
                    if ui
                        .button("Import Component…")
                        .on_hover_text("Load a .otdc into this network, linked to its file")
                        .clicked()
                    {
                        self.import_component();
                        ui.close();
                    }
                    ui.separator();
                    ui.menu_button("Examples", |ui| {
                        for name in otd_engine::demo::NAMES {
                            if ui.button(*name).clicked() {
                                self.load_demo(name);
                                ui.close();
                            }
                        }
                    });
                    ui.menu_button("Palette", |ui| {
                        for item in otd_engine::palette::ITEMS {
                            if ui.button(item.name).on_hover_text(item.summary).clicked() {
                                self.edit("palette");
                                let id = item.build(&mut self.graph, &self.registry, self.current);
                                self.select_only(id);
                                self.status = format!("added {} from the palette", item.name);
                                ui.close();
                            }
                        }
                    });
                });
                self.media_menu(ui);

                // Undo is on the bar, not only in the menu and the shortcut.
                // It is the one thing reached for in a hurry and while looking
                // at the network rather than at the menu, and a step you
                // cannot see how to take is a step you do not take. Disabled
                // when there is nothing to undo, so the bar also answers
                // "is there anything to go back to".
                ui.separator();
                let (back, ahead) = self.history.depth();
                if ui
                    .add_enabled(back > 0, egui::Button::new("↶"))
                    .on_hover_text("Undo (Cmd+Z)")
                    .clicked()
                {
                    self.undo();
                }
                if ui
                    .add_enabled(ahead > 0, egui::Button::new("↷"))
                    .on_hover_text("Redo (Cmd+Shift+Z)")
                    .clicked()
                {
                    self.redo();
                }

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
                // Breadcrumb: click a step to go back to it.
                let mut chain = Vec::new();
                let mut cur = Some(self.current);
                while let Some(c) = cur {
                    chain.push(c);
                    cur = self.graph.get(c).and_then(|n| n.parent);
                }
                chain.reverse();
                for (i, id) in chain.iter().enumerate() {
                    if i > 0 {
                        ui.label(egui::RichText::new("/").weak());
                    }
                    let name = if *id == self.graph.root() {
                        "/".to_string()
                    } else {
                        self.graph.node(*id).name.clone()
                    };
                    let last = i + 1 == chain.len();
                    let text = if last {
                        egui::RichText::new(name).strong()
                    } else {
                        egui::RichText::new(name).weak()
                    };
                    if ui.selectable_label(false, text).clicked() {
                        self.current = *id;
                        self.clear_selection();
                    }
                }

                ui.separator();
                if ui
                    .button("Perform")
                    .on_hover_text("Hide the editor and show only the output (F1)")
                    .clicked()
                {
                    self.perform = true;
                }
                ui.toggle_value(&mut self.assistant.bar, "✨ Assistant")
                    .on_hover_text("Describe a patch and have it built here — Cmd/Ctrl+K");
                ui.toggle_value(&mut self.show_monitor, "Perf")
                    .on_hover_text("Per-node cook cost and GPU memory");
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
                    self.engines.top.passes_this_frame,
                    (self.engines.top.resident_bytes() + self.engines.top.pooled_bytes()) as f64
                        / 1.0e6
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

    /// Your own material, and the things to do to it.
    ///
    /// Everything here is reachable another way — Tab finds any operator, and
    /// a file parameter has a Browse button. It is a menu because the other
    /// ways all require knowing the name of an operator first.
    fn media_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Media", |ui| {
            if ui
                .button("Import Media…")
                .on_hover_text("Movies, images and audio — or just drop the files on the network")
                .clicked()
            {
                crate::media::import_dialog(self);
                ui.close();
            }
            if ui
                .button("Use Webcam")
                .on_hover_text("A Video Device In, viewing straight away")
                .clicked()
            {
                crate::media::add_webcam(self);
                ui.close();
            }
            ui.separator();

            let selected = self
                .selected
                .filter(|s| self.graph.contains(*s))
                .map(|s| self.graph.node(s).name.clone());
            let label = match &selected {
                Some(name) => format!("Add Effect after {name}"),
                None => "Add Effect".to_string(),
            };
            ui.add_enabled_ui(selected.is_some(), |ui| {
                ui.menu_button(label, |ui| crate::media::effects_menu(self, ui));
            });
            if selected.is_none() {
                ui.label(
                    egui::RichText::new("select an operator to put an effect after it")
                        .weak()
                        .small(),
                );
            }
        });
    }

    /// The performance monitor: what each node costs, and what the GPU holds.
    ///
    /// Ranked by cost *per frame* rather than by cook time. Those are not the
    /// same number in a demand-driven engine, and ranking by the wrong one
    /// sends you off optimising a node that cooks once and then sits there.
    fn monitor(&mut self, ctx: &egui::Context) {
        if !self.show_monitor {
            return;
        }
        let mut open = self.show_monitor;
        egui::Window::new("Performance")
            .open(&mut open)
            .default_width(420.0)
            .default_height(360.0)
            .show(ctx, |ui| {
                let budget = 1000.0 / self.time.fps.max(1.0);
                let frame_ms = 1000.0 / self.smoothed_fps.max(1e-6);
                ui.horizontal(|ui| {
                    ui.strong(format!("{frame_ms:.2} ms/frame"));
                    ui.label(
                        egui::RichText::new(format!("budget {budget:.2} ms"))
                            .weak()
                            .small(),
                    );
                    let (colour, verdict) = if frame_ms <= budget {
                        (egui::Color32::from_rgb(130, 200, 140), "holding the rate")
                    } else {
                        (egui::Color32::from_rgb(235, 120, 120), "over budget")
                    };
                    ui.colored_label(colour, egui::RichText::new(verdict).small());
                });
                ui.label(
                    egui::RichText::new(format!(
                        "cook {:.2} ms · {} cooked, {} cached last frame",
                        self.smoothed_cook_ms, self.cook.stats.cooked, self.cook.stats.cached
                    ))
                    .weak()
                    .small(),
                );

                let top = self.engines.top.pooled_bytes();
                let resident = self.engines.top.resident_bytes();
                ui.label(
                    egui::RichText::new(format!(
                        "GPU  {} in node outputs · {} pooled · {} textures created",
                        bytes(resident),
                        bytes(top),
                        self.engines.top.textures_created()
                    ))
                    .weak()
                    .small(),
                );
                ui.separator();

                // Every node that has cooked, most expensive per frame first.
                let mut rows: Vec<(NodeId, f64, f64, f64)> = self
                    .graph
                    .walk()
                    .into_iter()
                    .filter(|id| *id != self.graph.root())
                    .map(|id| {
                        (
                            id,
                            self.cook.frame_cost_ms(id),
                            self.cook.avg_cook_ms(id),
                            self.cook.cook_rate(id),
                        )
                    })
                    .filter(|(_, _, avg, _)| *avg > 0.0)
                    .collect();
                rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

                if rows.is_empty() {
                    ui.label(egui::RichText::new("nothing has cooked yet").weak());
                    return;
                }
                let worst = rows[0].1.max(1e-6);

                let mut jump = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::Grid::new("perf-rows")
                        .num_columns(4)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("node").small().strong());
                            ui.label(egui::RichText::new("ms/frame").small().strong());
                            ui.label(egui::RichText::new("per cook").small().strong());
                            ui.label(egui::RichText::new("cooks").small().strong());
                            ui.end_row();

                            for (id, cost, avg, rate) in &rows {
                                let path = self.graph.path(*id);
                                if ui.selectable_label(false, &path).clicked() {
                                    jump = Some(*id);
                                }
                                // Tint by share of the worst offender, so the
                                // eye lands on the problem without reading.
                                let heat = (cost / worst) as f32;
                                ui.colored_label(
                                    egui::Color32::from_rgb(
                                        (150.0 + 85.0 * heat) as u8,
                                        (200.0 - 80.0 * heat) as u8,
                                        (160.0 - 60.0 * heat) as u8,
                                    ),
                                    egui::RichText::new(format!("{cost:.3}")).monospace(),
                                );
                                ui.label(
                                    egui::RichText::new(format!("{avg:.3}")).monospace().weak(),
                                );
                                ui.label(
                                    egui::RichText::new(format!("{:.0}%", rate * 100.0))
                                        .monospace()
                                        .weak(),
                                );
                                ui.end_row();
                            }
                        });
                });
                // Clicking a row selects the node, so the monitor is a way in
                // rather than a wall of numbers to go hunting from.
                if let Some(id) = jump {
                    self.select_only(id);
                }
            });
        self.show_monitor = open;
    }

    /// The timeline strip: a scrubbable playhead and the loop range.
    ///
    /// The playhead is the same `CookContext` the whole network reads, so
    /// dragging it drags the entire patch — every time-dependent operator, the
    /// keyframe curves, the LFOs. There is no separate "timeline time" that
    /// could get out of step with what is rendering.
    fn timeline(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("timeline")
            .exact_size(52.0)
            .show(ui, |ui| {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    let icon = if self.playing { "⏸" } else { "▶" };
                    if ui
                        .button(icon)
                        .on_hover_text("Play / pause (Space)")
                        .clicked()
                    {
                        self.playing = !self.playing;
                    }
                    if ui
                        .button("⏮")
                        .on_hover_text("Back to the loop start")
                        .clicked()
                    {
                        self.time.time = self.loop_range.0;
                        self.time.frame = (self.loop_range.0 * self.time.fps).round() as i64;
                    }
                    ui.toggle_value(&mut self.looping, "Loop");

                    ui.label("from");
                    ui.add(
                        egui::DragValue::new(&mut self.loop_range.0)
                            .speed(0.05)
                            .range(0.0..=3600.0)
                            .suffix("s"),
                    );
                    ui.label("to");
                    ui.add(
                        egui::DragValue::new(&mut self.loop_range.1)
                            .speed(0.05)
                            .range(0.0..=3600.0)
                            .suffix("s"),
                    );
                    // An inverted range would make the wrap arithmetic
                    // meaningless; keep the end after the start rather than
                    // guarding at every use.
                    if self.loop_range.1 <= self.loop_range.0 {
                        self.loop_range.1 = self.loop_range.0 + 0.1;
                    }

                    ui.label(
                        egui::RichText::new(format!(
                            "frame {}    {:.2}s",
                            self.time.frame, self.time.time
                        ))
                        .monospace(),
                    );
                });
                self.playhead(ui);
            });
    }

    /// The scrub bar. Click or drag anywhere on it to move time.
    fn playhead(&mut self, ui: &mut egui::Ui) {
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 16.0),
            egui::Sense::click_and_drag(),
        );
        let (start, end) = self.loop_range;
        let span = (end - start).max(1e-6);

        let painter = ui.painter();
        painter.rect_filled(rect, 3.0, egui::Color32::from_gray(30));

        // Second ticks, as long as they are not so dense as to be a smear.
        let seconds = span.ceil() as i64;
        if seconds <= 120 {
            for i in 0..=seconds {
                let t = start.floor() + i as f64;
                if t < start || t > end {
                    continue;
                }
                let x = rect.left() + ((t - start) / span) as f32 * rect.width();
                painter.line_segment(
                    [
                        egui::pos2(x, rect.bottom() - 5.0),
                        egui::pos2(x, rect.bottom()),
                    ],
                    egui::Stroke::new(1.0, egui::Color32::from_gray(70)),
                );
            }
        }

        if resp.dragged() || resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let u = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
                self.time.time = start + u * span;
                self.time.frame = (self.time.time * self.time.fps).round() as i64;
                // Scrubbing owns time for this frame: letting the transport
                // also advance would fight the drag and jitter the playhead.
                self.scrubbing = true;
            }
        }

        let u = ((self.time.time - start) / span).clamp(0.0, 1.0) as f32;
        let x = rect.left() + u * rect.width();
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(2.0, egui::Color32::from_rgb(230, 170, 90)),
        );
    }

    /// Perform mode: the main window becomes the output.
    ///
    /// This is not a different renderer or a different cook — it is the same
    /// frame with the editor's panels not drawn. What it buys is what the
    /// editor costs: no thumbnails to sample, no canvas to lay out, and every
    /// node that was only cooking because it was visible on the canvas stops
    /// cooking, because `cook_roots` asks what is on screen and the answer is
    /// now nothing but the viewer.
    fn perform_view(&mut self, ui: &mut egui::Ui) {
        let widgets = otd_engine::panel::widgets(&self.graph);
        let mut panels: Vec<(NodeId, f64)> = Vec::new();
        // Nothing is on the canvas in perform mode, so nothing is a visible
        // cook root. Clearing this is what makes the mode cheaper rather than
        // merely darker.
        self.visible.clear();

        let tex = self
            .viewer
            .filter(|v| self.graph.contains(*v))
            .and_then(|v| self.thumbnail(v));
        egui::CentralPanel::no_frame()
            .frame(egui::Frame::NONE.fill(egui::Color32::BLACK))
            .show(ui, |ui| {
                let area = match tex {
                    Some((tid, size)) => letterbox(ui, tid, size),
                    None => {
                        ui.centered_and_justified(|ui| {
                            ui.label(
                                egui::RichText::new("no viewer — F1 to go back")
                                    .color(egui::Color32::from_gray(90)),
                            );
                        });
                        // With no output to lay them against, the widgets get
                        // the whole pane rather than disappearing: a panel you
                        // cannot see is indistinguishable from one that is
                        // broken.
                        ui.max_rect()
                    }
                };
                panels.extend(draw_panel(ui, &widgets, area));
            });

        // Applied after the panel is drawn, because the write needs the graph
        // mutably and drawing borrowed it.
        for (id, value) in panels {
            self.edit("panel");
            let _ = self
                .graph
                .set_param(id, "value", otd_core::Value::Float(value));
        }
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
        let widgets = otd_engine::panel::widgets(&self.graph);
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
                            let area = letterbox(ui, tid, size);
                            // Drawn, not driven: the output window is often on
                            // a projector where nobody is holding a mouse, and
                            // two surfaces both writing the same parameter in
                            // one frame is a fight rather than a feature.
                            let _ = draw_panel(ui, &widgets, area);
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

/// What has to cook this frame: the big viewer, every node flagged for render,
/// and every node whose own viewer is on *and* which is currently on screen.
///
/// That last condition is what makes the demand-driven engine visible: pan a
/// heavy branch off the canvas and it stops costing anything. It is also what
/// makes perform mode cheap rather than merely dark — nothing is on the canvas
/// there, so `visible` is empty and the only thing left is the output chain.
fn cook_roots(graph: &Graph, viewer: Option<NodeId>, visible: &[NodeId]) -> Vec<NodeId> {
    let mut roots = Vec::new();
    if let Some(v) = viewer.filter(|v| graph.contains(*v)) {
        roots.push(v);
    }
    for id in graph.walk() {
        if id == graph.root() {
            continue;
        }
        let node = graph.node(id);
        // An Execute DAT is a root by its nature — see `otd_engine::execute`.
        let wanted = node.flags.render
            || otd_engine::execute::is_execute(&node.op_type)
            || (node.flags.display && visible.contains(&id));
        if wanted && !roots.contains(&id) {
            roots.push(id);
        }
    }
    roots
}

/// Bytes, in whatever unit keeps the number short.
fn bytes(n: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    if n as f64 >= MB {
        format!("{:.1} MB", n as f64 / MB)
    } else {
        format!("{:.0} kB", n as f64 / 1024.0)
    }
}

/// Paint a texture centred and letterboxed.
///
/// Letterbox rather than stretch, in both the output window and perform mode:
/// a show output at the wrong aspect ratio is worse than black bars, and worse
/// still because it looks deliberate.
/// Returns the rect it painted into, which is the box a panel lays out
/// against — the picture, not the window, so a widget stays where it was put
/// relative to the image whatever shape the window is.
fn letterbox(ui: &egui::Ui, tid: TextureId, size: [u32; 2]) -> egui::Rect {
    let avail = ui.available_size();
    let aspect = size[0] as f32 / size[1].max(1) as f32;
    let (mut w, mut h) = (avail.x, avail.x / aspect);
    if h > avail.y {
        h = avail.y;
        w = h * aspect;
    }
    let rect = egui::Rect::from_center_size(ui.max_rect().center(), egui::vec2(w, h));
    ui.painter().image(
        tid,
        rect,
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
    rect
}

/// Draw the panel widgets over `area`, returning the values a person changed.
///
/// The edits come back rather than being applied here: writing needs the graph
/// mutably, and the caller is already holding it to have drawn at all. Keeping
/// that split also means this function is only about pixels.
fn draw_panel(
    ui: &mut egui::Ui,
    widgets: &[otd_engine::panel::Widget],
    area: egui::Rect,
) -> Vec<(NodeId, f64)> {
    use otd_engine::panel::Kind;
    let mut changed = Vec::new();
    for w in widgets {
        let rect = egui::Rect::from_min_size(
            area.min + egui::vec2(w.rect[0] * area.width(), w.rect[1] * area.height()),
            egui::vec2(w.rect[2] * area.width(), w.rect[3] * area.height()),
        );
        if !ui.max_rect().intersects(rect) {
            continue;
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        match w.kind {
            Kind::Button => {
                let on = w.value >= 0.5;
                let text =
                    egui::RichText::new(&w.label).size((rect.height() * 0.4).clamp(9.0, 28.0));
                let response = child.put(rect, egui::Button::new(text).selected(on));
                if w.momentary {
                    // Held, not latched: down while the pointer is down, and
                    // released when it lets go, wherever it lets go.
                    let down = response.is_pointer_button_down_on();
                    if down != on {
                        changed.push((w.id, if down { 1.0 } else { 0.0 }));
                    }
                } else if response.clicked() {
                    changed.push((w.id, if on { 0.0 } else { 1.0 }));
                }
            }
            Kind::Slider => {
                let (lo, hi) = w.range;
                let mut value = w.value;
                let response = if w.vertical {
                    child.put(
                        rect,
                        egui::Slider::new(&mut value, lo..=hi)
                            .vertical()
                            .show_value(false),
                    )
                } else {
                    child.put(
                        rect,
                        egui::Slider::new(&mut value, lo..=hi).show_value(false),
                    )
                };
                if response.changed() {
                    changed.push((w.id, value));
                }
            }
            Kind::Field => {
                // Read-only here: a text field's value is its Text parameter,
                // and editing text on the *output* is a different feature from
                // showing it. Typing into it belongs to the parameter panel
                // until there is a reason it does not.
                child.put(
                    rect,
                    egui::Label::new(
                        egui::RichText::new(&w.text).size((rect.height() * 0.4).clamp(9.0, 28.0)),
                    ),
                );
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Noise -> level -> out, plus an unrelated branch nobody wired up.
    fn patch() -> (Graph, OpRegistry, NodeId, NodeId) {
        let registry = otd_engine::registry();
        let mut graph = Graph::new();
        let root = graph.root();
        let noise = graph
            .create(root, registry.get("noiseTOP").unwrap(), Some("noise1"))
            .unwrap();
        let out = graph
            .create(root, registry.get("nullTOP").unwrap(), Some("out1"))
            .unwrap();
        graph.connect(noise, out, 0).unwrap();
        let spare = graph
            .create(root, registry.get("rampTOP").unwrap(), Some("ramp1"))
            .unwrap();
        (graph, registry, out, spare)
    }

    #[test]
    fn a_node_on_screen_cooks_and_the_same_node_off_screen_does_not() {
        let (graph, _reg, out, spare) = patch();

        // On the canvas: its own viewer makes it a root.
        let roots = cook_roots(&graph, Some(out), &[spare]);
        assert!(roots.contains(&spare));

        // Panned away: nothing wants it, so nothing cooks it.
        let roots = cook_roots(&graph, Some(out), &[]);
        assert!(!roots.contains(&spare));
        assert_eq!(roots, vec![out], "only the viewer chain is left");
    }

    #[test]
    fn perform_mode_leaves_only_the_output_chain() {
        let (mut graph, _reg, out, spare) = patch();
        // A render-flagged node is a root wherever the canvas is looking —
        // that is what the flag means, and a show output relies on it.
        graph.node_mut(spare).flags.render = true;

        // Perform mode is exactly "nothing is on the canvas".
        let performing = cook_roots(&graph, Some(out), &[]);
        assert!(performing.contains(&out));
        assert!(
            performing.contains(&spare),
            "the render flag survives perform mode"
        );

        let noise = graph.find("/noise1").unwrap();
        assert!(
            !performing.contains(&noise),
            "an upstream node is pulled by its consumer, not listed as a root"
        );
    }

    #[test]
    fn a_deleted_viewer_does_not_become_a_root() {
        let (mut graph, _reg, out, _) = patch();
        graph.remove(out).unwrap();
        // A stale NodeId from a deleted node must not reach the cook.
        assert!(cook_roots(&graph, Some(out), &[]).is_empty());
    }
}
