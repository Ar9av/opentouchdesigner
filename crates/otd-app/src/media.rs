//! Getting your own material into a patch.
//!
//! The shortest path from "I have a video" to "the video is on screen" is to
//! drop the file on the network and have it already playing. That is what this
//! module is: one classifier from a file extension to the operator that plays
//! it, and the three ways of reaching it — drag and drop, the Media menu, and
//! the Browse button on a file parameter.
//!
//! Nothing here asks the user what an operator is. Drop a movie and you get a
//! Movie File In wired to the viewer; drop a shader and you get a GLSL TOP with
//! the source in it; drop a `.otd` and the project opens. The node graph is
//! still the thing you end up in, but you do not have to know it to start.

use std::path::{Path, PathBuf};

use otd_core::{NodeId, Value};

use crate::app::OtdApp;

/// What a file becomes when it lands on the network.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropKind {
    /// A still — PNG, JPEG, and friends. Movie File In plays these too.
    Image,
    Movie,
    Audio,
    /// A whole project. Opens it, rather than adding anything.
    Project,
    /// A `.otdc` component, linked to its file.
    Component,
    /// An ISF shader: a GLSL TOP with its inputs turned into parameters.
    Isf,
    /// Plain shader source for a GLSL TOP.
    Shader(&'static str),
    /// Delimited text — a Table DAT.
    Table(&'static str),
    /// Anything else textual — a Text DAT.
    Text,
    /// A font file, for a Text TOP.
    Font,
    Unknown,
}

impl DropKind {
    /// What the user is about to get, in their words rather than the
    /// registry's. Shown on the drop overlay before they let go.
    pub fn describe(self) -> &'static str {
        match self {
            DropKind::Image => "Movie File In — a still",
            DropKind::Movie => "Movie File In — plays on the timeline",
            DropKind::Audio => "Audio File In — a CHOP you can drive parameters with",
            DropKind::Project => "open this project",
            DropKind::Component => "import this component",
            DropKind::Isf => "GLSL TOP — ISF inputs become parameters",
            DropKind::Shader(_) => "GLSL TOP — the source, compiled live",
            DropKind::Table(_) => "Table DAT",
            DropKind::Text => "Text DAT",
            DropKind::Font => "Text TOP, in this font",
            DropKind::Unknown => "not a file this can open",
        }
    }

    pub fn is_media(self) -> bool {
        matches!(self, DropKind::Image | DropKind::Movie | DropKind::Audio)
    }
}

/// Extensions the file dialogs offer, so Browse and drag-and-drop agree on
/// what counts as media.
pub const IMAGE_EXT: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "tga", "tif", "tiff"];
pub const MOVIE_EXT: &[&str] = &["mp4", "mov", "m4v", "mkv", "webm", "avi", "gif"];
pub const AUDIO_EXT: &[&str] = &["wav", "mp3", "aiff", "aif", "flac", "ogg", "m4a", "aac"];

/// One file, one operator. Extension only: reading headers to identify a file
/// the user just chose is effort spent to disagree with them.
pub fn classify(path: &Path) -> DropKind {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        e if IMAGE_EXT.contains(&e) => DropKind::Image,
        e if MOVIE_EXT.contains(&e) => DropKind::Movie,
        e if AUDIO_EXT.contains(&e) => DropKind::Audio,
        "otd" => DropKind::Project,
        "otdc" => DropKind::Component,
        "fs" | "isf" => DropKind::Isf,
        "frag" | "glsl" => DropKind::Shader("glsl"),
        "wgsl" => DropKind::Shader("wgsl"),
        "csv" => DropKind::Table("comma"),
        "tsv" => DropKind::Table("tab"),
        "txt" | "md" | "json" | "py" | "log" => DropKind::Text,
        "ttf" | "otf" => DropKind::Font,
        _ => DropKind::Unknown,
    }
}

/// Effects offered as "add this on top of what is selected". A short list on
/// purpose: the full 126 operators are a Tab away, and a menu of everything is
/// a menu nobody reads.
pub const EFFECTS: &[&str] = &[
    "levelTOP",
    "blurTOP",
    "hsvadjustTOP",
    "edgeTOP",
    "thresholdTOP",
    "mirrorTOP",
    "transformTOP",
    "flipTOP",
    "chromakeyTOP",
    "displaceTOP",
    "lookupTOP",
    "compositeTOP",
    "feedbackTOP",
];

// ------------------------------------------------------------------ the loop

/// Called once a frame. Reads what the window system dropped on us and turns
/// it into operators.
pub fn handle_drops(app: &mut OtdApp, ctx: &egui::Context) {
    let dropped: Vec<PathBuf> = ctx.input(|i| {
        i.raw
            .dropped_files
            .iter()
            .filter_map(|f| f.path.clone())
            .collect()
    });
    if dropped.is_empty() {
        return;
    }
    let at = ctx.pointer_latest_pos();
    // An image let go over the assistant bar is a reference to build *from*,
    // not material to build *with*. Anywhere else on the window it is still a
    // Movie File In, which is what dropping a picture on a node graph means.
    if crate::assistant::claim_drop(app, &dropped, at) {
        return;
    }
    accept(app, &dropped, at);
}

/// The translucent "let go and you get this" overlay, drawn while files are
/// held over the window. Naming the operator before the drop is what stops the
/// drop from being a guess.
pub fn hover_overlay(app: &OtdApp, ctx: &egui::Context) {
    let hovered: Vec<PathBuf> = ctx.input(|i| {
        i.raw
            .hovered_files
            .iter()
            .filter_map(|f| f.path.clone())
            .collect()
    });
    if hovered.is_empty() {
        return;
    }
    let rect = if app.canvas_rect.is_positive() {
        app.canvas_rect
    } else {
        ctx.input(|i| i.raw.screen_rect.unwrap_or(egui::Rect::ZERO))
    };
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("drop-overlay"),
    ));
    painter.rect_filled(rect, 0.0, egui::Color32::from_black_alpha(170));
    painter.rect_stroke(
        rect.shrink(6.0),
        8.0,
        egui::Stroke::new(2.0, egui::Color32::from_rgb(120, 180, 240)),
        egui::StrokeKind::Inside,
    );

    // Held over the assistant bar, the same file means something else, and
    // the overlay has to say which one is about to happen.
    let as_reference = crate::assistant::would_claim(app, &hovered, ctx.pointer_latest_pos());

    let mut y = rect.center().y - (hovered.len().min(8) as f32 * 24.0) / 2.0 - 22.0;
    painter.text(
        egui::pos2(rect.center().x, y),
        egui::Align2::CENTER_CENTER,
        match as_reference {
            true => "Drop to build this look",
            false => "Drop to add",
        },
        egui::FontId::proportional(20.0),
        egui::Color32::from_rgb(200, 220, 245),
    );
    y += 34.0;
    for path in hovered.iter().take(8) {
        let kind = classify(path);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let colour = if kind == DropKind::Unknown && !as_reference {
            egui::Color32::from_rgb(230, 130, 130)
        } else {
            egui::Color32::from_rgb(225, 228, 235)
        };
        let becomes = match as_reference {
            true => "reference image for the assistant",
            false => kind.describe(),
        };
        painter.text(
            egui::pos2(rect.center().x, y),
            egui::Align2::CENTER_CENTER,
            format!("{name}   ->   {becomes}"),
            egui::FontId::proportional(15.0),
            colour,
        );
        y += 24.0;
    }
    if hovered.len() > 8 {
        painter.text(
            egui::pos2(rect.center().x, y),
            egui::Align2::CENTER_CENTER,
            format!("…and {} more", hovered.len() - 8),
            egui::FontId::proportional(13.0),
            egui::Color32::from_gray(150),
        );
    }
}

// -------------------------------------------------------------- the dialogs

/// File ▸ Import Media…, and the button on an empty network.
pub fn import_dialog(app: &mut OtdApp) {
    let mut all: Vec<&str> = Vec::new();
    all.extend_from_slice(IMAGE_EXT);
    all.extend_from_slice(MOVIE_EXT);
    all.extend_from_slice(AUDIO_EXT);
    let picked = rfd::FileDialog::new()
        .set_title("Import media")
        .add_filter("Movies, images and audio", &all)
        .add_filter("Movies", MOVIE_EXT)
        .add_filter("Images", IMAGE_EXT)
        .add_filter("Audio", AUDIO_EXT)
        .add_filter("Anything", &["*"])
        .pick_files();
    if let Some(paths) = picked {
        accept(app, &paths, None);
    }
}

/// A camera, one click. The other half of "use your own material" for anyone
/// who has not got a file to hand.
pub fn add_webcam(app: &mut OtdApp) {
    let pos = default_pos(app);
    if let Some(id) = create_named(app, otd_gpu::ops::VIDEO_DEVICE_IN, pos, "camera") {
        app.viewer = Some(id);
        app.status =
            "Video Device In — the first frame arrives once the camera permission is granted"
                .into();
    }
}

/// The effects list, as menu rows. Shared by the Media menu and the node's
/// own right-click menu so the two cannot drift apart.
pub fn effects_menu(app: &mut OtdApp, ui: &mut egui::Ui) {
    let effects: Vec<(&'static str, &'static str, &'static str)> = EFFECTS
        .iter()
        .filter_map(|t| app.registry.get(t))
        .map(|d| (d.type_name, d.label, d.summary))
        .collect();
    for (type_name, label, summary) in effects {
        if ui.button(label).on_hover_text(summary).clicked() {
            add_effect(app, type_name);
            ui.close();
        }
    }
}

/// The file dialog for one operator's file parameter, filtered to what that
/// operator can actually read — or, for a recorder, asking where to write.
pub fn pick_for(op_type: &str, key: &str) -> Option<PathBuf> {
    if op_type == otd_gpu::ops::MOVIE_OUT {
        return rfd::FileDialog::new()
            .set_title("Record to")
            .add_filter("Movie", MOVIE_EXT)
            .set_file_name("out.mp4")
            .save_file();
    }
    let dialog = rfd::FileDialog::new().set_title("Choose a file");
    let dialog = match (op_type, key) {
        (otd_chop::io::AUDIO_FILE, _) => dialog.add_filter("Audio (WAV)", &["wav"]),
        (_, "font") => dialog.add_filter("Font", &["ttf", "otf"]),
        (otd_gpu::ops::MOVIE_IN, _) => {
            let mut all: Vec<&str> = Vec::new();
            all.extend_from_slice(MOVIE_EXT);
            all.extend_from_slice(IMAGE_EXT);
            dialog.add_filter("Movies and images", &all)
        }
        _ => dialog,
    };
    dialog.add_filter("Anything", &["*"]).pick_file()
}

/// Swap the file a node plays, filtered to what that node can read.
pub fn replace_file(app: &mut OtdApp, id: NodeId) {
    let op_type = app.graph.node(id).op_type.clone();
    let Some(path) = pick_for(&op_type, "file") else {
        return;
    };
    app.edit("set file");
    app.history.end_gesture();
    let stored = stored_path(app, &path);
    let _ = app.graph.set_param(id, "file", Value::Str(stored));
    app.viewer = Some(id);
    app.status = format!(
        "{} -> {}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        app.graph.node(id).name
    );
}

/// Add an operator downstream of whatever is selected and view the result:
/// the "do something to it" half of dropping a clip in.
pub fn add_effect(app: &mut OtdApp, type_name: &str) {
    let Some(src) = app.selected.filter(|s| app.graph.contains(*s)) else {
        app.status = "select an operator first — the effect goes after it".into();
        return;
    };
    let p = app.graph.node(src).pos;
    let pos = free_spot(app, egui::vec2(p[0] + crate::canvas::NODE_W + 40.0, p[1]));
    let Some(id) = app.create_node(type_name, pos) else {
        return;
    };
    // No checkpoint of its own: `create_node` took one, and the wire is part
    // of the same action — undoing the effect has to take its wire with it.
    match app.graph.connect(src, id, 0) {
        Ok(()) => {
            app.viewer = Some(id);
            app.status = format!(
                "{} -> {}",
                app.graph.node(src).name,
                app.graph.node(id).name
            );
        }
        Err(e) => app.status = format!("{} does not take that input: {e}", app.graph.node(id).name),
    }
}

// --------------------------------------------------------------- the routing

/// Turn a list of files into operators. `at` is where the pointer was, in
/// screen space; without one the files land in the middle of the view.
pub fn accept(app: &mut OtdApp, paths: &[PathBuf], at: Option<egui::Pos2>) {
    // A project replaces everything, so it can only sensibly be alone.
    if let Some(project) = paths.iter().find(|p| classify(p) == DropKind::Project) {
        if paths.len() > 1 {
            app.status = format!(
                "opened {} and ignored the rest — a project replaces the network",
                project.display()
            );
        }
        let project = project.clone();
        app.open(Some(project));
        return;
    }

    // Dropping straight onto a node is an edit of that node, not a new one:
    // swapping the clip in a movie player you already wired up is the common
    // case, and making a second player for it would be wrong.
    if let (Some(pointer), 1) = (at, paths.len()) {
        if let Some(target) = crate::canvas::node_at(app, pointer) {
            if retarget(app, target, &paths[0]) {
                return;
            }
        }
    }

    let base = match at {
        Some(p) if app.canvas_rect.contains(p) => app.view.to_world(app.canvas_rect.min, p),
        _ => default_pos(app),
    };

    let mut added = 0usize;
    let mut skipped: Vec<String> = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        let pos = free_spot(
            app,
            base + egui::vec2(0.0, i as f32 * (crate::canvas::NODE_H + 28.0)),
        );
        match add_one(app, path, pos) {
            Some(id) => {
                added += 1;
                app.selected = Some(id);
            }
            None => skipped.push(
                path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
        }
    }

    if !skipped.is_empty() {
        app.status = format!(
            "ignored {} — movies, images, audio, shaders, tables, fonts, .otd and .otdc are what \
             this reads",
            skipped.join(", ")
        );
    } else if added > 1 {
        app.status = format!("added {added} operators");
    }
}

/// One file, one operator, at a known place. `None` means the file was not
/// something we know what to do with.
fn add_one(app: &mut OtdApp, path: &Path, pos: egui::Vec2) -> Option<NodeId> {
    let kind = classify(path);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "media".into());
    let stored = stored_path(app, path);

    match kind {
        DropKind::Image | DropKind::Movie => {
            let id = create_named(app, otd_gpu::ops::MOVIE_IN, pos, &stem)?;
            let _ = app.graph.set_param(id, "file", Value::Str(stored));
            // Show it. Somebody who just dropped a clip wants to see the clip,
            // not to be told which node to double-click.
            app.viewer = Some(id);
            app.status = format!(
                "{} — drop another on this node to swap the clip",
                app.graph.node(id).name
            );
            Some(id)
        }
        DropKind::Audio => {
            let id = create_named(app, otd_chop::io::AUDIO_FILE, pos, &stem)?;
            let _ = app.graph.set_param(id, "file", Value::Str(stored));
            app.status = if path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("wav"))
                .unwrap_or(false)
            {
                format!(
                    "{} — drag a channel from the parameter panel onto any parameter",
                    app.graph.node(id).name
                )
            } else {
                // Say it here rather than leaving a silent node: the operator
                // reads WAV only, and the file dialog cannot know that for a
                // file the user dragged in from elsewhere.
                format!(
                    "{}: Audio File In reads WAV only — convert it, or use the movie's own audio",
                    app.graph.node(id).name
                )
            };
            Some(id)
        }
        DropKind::Component => {
            let id = app.import_component_at(path)?;
            app.graph.node_mut_quiet(id).pos = [pos.x, pos.y];
            Some(id)
        }
        DropKind::Isf => {
            let source = read_text(path)?;
            let isf = otd_gpu::isf::import(&source)
                .map_err(|e| app.status = format!("ISF import failed: {e}"))
                .ok()?;
            let id = create_named(app, otd_gpu::ops::GLSL, pos, &stem)?;
            let count = isf.params.len();
            otd_gpu::isf::apply(&mut app.graph, id, &isf);
            app.viewer = Some(id);
            app.status = format!("{stem} — {count} parameter(s)");
            Some(id)
        }
        DropKind::Shader(language) => {
            let source = read_text(path)?;
            let id = create_named(app, otd_gpu::ops::GLSL, pos, &stem)?;
            let _ = app
                .graph
                .set_param(id, "language", Value::Str(language.into()));
            let _ = app.graph.set_param(id, "source", Value::Str(source));
            app.viewer = Some(id);
            Some(id)
        }
        DropKind::Table(delimiter) => {
            let text = read_text(path)?;
            let id = create_named(app, otd_dat::ops::TABLE, pos, &stem)?;
            let _ = app.graph.set_param(id, "text", Value::Str(text));
            let _ = app
                .graph
                .set_param(id, "delimiter", Value::Str(delimiter.into()));
            Some(id)
        }
        DropKind::Text => {
            let text = read_text(path)?;
            let id = create_named(app, otd_dat::ops::TEXT, pos, &stem)?;
            let _ = app.graph.set_param(id, "text", Value::Str(text));
            Some(id)
        }
        DropKind::Font => {
            // A font on its own is not a picture; it is a picture of some
            // text, so the drop makes the Text TOP that draws it.
            let id = create_named(app, otd_gpu::ops::TEXT, pos, &stem)?;
            let _ = app.graph.set_param(id, "font", Value::Str(stored));
            let _ = app.graph.set_param(id, "text", Value::Str(stem));
            app.viewer = Some(id);
            Some(id)
        }
        DropKind::Project | DropKind::Unknown => None,
    }
}

/// Drop onto an existing node. Returns whether the node took the file.
///
/// A file parameter of the right sort is replaced in place; otherwise, if the
/// node has a free texture input, the media is made *upstream* of it and wired
/// in — dropping a clip onto a blur means "blur this clip".
fn retarget(app: &mut OtdApp, target: NodeId, path: &Path) -> bool {
    let kind = classify(path);
    if !kind.is_media() {
        return false;
    }
    let op_type = app.graph.node(target).op_type.clone();
    let stored = stored_path(app, path);

    let takes_file = matches!(
        (op_type.as_str(), kind),
        (otd_gpu::ops::MOVIE_IN, DropKind::Image | DropKind::Movie)
            | (otd_chop::io::AUDIO_FILE, DropKind::Audio)
    );
    if takes_file {
        app.edit("set file");
        app.history.end_gesture();
        let _ = app.graph.set_param(target, "file", Value::Str(stored));
        app.viewer = Some(target);
        app.selected = Some(target);
        app.status = format!(
            "{} -> {}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            app.graph.node(target).name
        );
        return true;
    }

    // Wire it in upstream instead, into the first input that is free and takes
    // what this file produces.
    let wanted = if kind == DropKind::Audio {
        otd_core::Family::Chop
    } else {
        otd_core::Family::Top
    };
    let node = app.graph.node(target);
    let slot = node.inputs.iter().enumerate().position(|(i, s)| {
        s.is_none() && node.input_families.get(i).copied().unwrap_or(node.family) == wanted
    });
    let Some(slot) = slot else {
        return false;
    };

    let p = app.graph.node(target).pos;
    let pos = free_spot(
        app,
        egui::vec2(
            p[0] - crate::canvas::NODE_W - 40.0,
            p[1] + slot as f32 * 24.0,
        ),
    );
    let Some(id) = add_one(app, path, pos) else {
        return false;
    };
    match app.graph.connect(id, target, slot) {
        Ok(()) => {
            // Keep the eye on the node being fed, not on the raw clip: the
            // point of dropping onto a blur is to see the blurred clip.
            app.viewer = Some(target);
            app.status = format!(
                "{} -> {}",
                app.graph.node(id).name,
                app.graph.node(target).name
            );
        }
        Err(e) => app.status = e.to_string(),
    }
    true
}

// ----------------------------------------------------------------- plumbing

/// How a path gets written into the project.
///
/// Relative when the file is inside the project's own folder, absolute
/// otherwise. That is the same rule `bundle::export` relies on, and it is what
/// makes a project folder you can move to the show machine.
pub fn stored_path(app: &OtdApp, path: &Path) -> String {
    if let Some(base) = app.graph.base_dir() {
        if let Ok(rel) = path.strip_prefix(base) {
            return rel.to_string_lossy().to_string();
        }
    }
    path.to_string_lossy().to_string()
}

/// Text files land in the project file itself, so there is a ceiling on how
/// much of one we will swallow.
const MAX_TEXT_BYTES: u64 = 4 * 1024 * 1024;

fn read_text(path: &Path) -> Option<String> {
    match std::fs::metadata(path) {
        Ok(m) if m.len() > MAX_TEXT_BYTES => return None,
        Ok(_) => {}
        Err(_) => return None,
    }
    std::fs::read_to_string(path).ok()
}

/// Create an operator and name it after the file, when that name is free and
/// is a legal operator name.
fn create_named(app: &mut OtdApp, type_name: &str, pos: egui::Vec2, hint: &str) -> Option<NodeId> {
    let id = app.create_node(type_name, pos)?;
    if let Some(name) = sanitise(hint) {
        let parent = app.graph.node(id).parent.unwrap_or(app.graph.root());
        if !app.graph.name_taken(parent, &name) {
            app.graph.node_mut_quiet(id).name = name;
        }
    }
    Some(id)
}

/// About what fits in a node header at 100% zoom. A camera roll or an export
/// from some other tool hands you names like
/// `chatgpt_image_aug_5_2026_11_59_18_pm`, and the whole point of naming the
/// node after the file is to be able to read it.
const MAX_NAME: usize = 20;

/// `My Clip (final).mp4` is not an operator name. `my_clip_final` is.
fn sanitise(hint: &str) -> Option<String> {
    let mut out = String::new();
    let mut last_underscore = false;
    for c in hint.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_underscore = false;
        } else if !last_underscore && !out.is_empty() {
            out.push('_');
            last_underscore = true;
        }
    }
    // Cut at a word boundary when there is one to cut at, so the name still
    // says what the file was rather than stopping mid-word.
    if out.len() > MAX_NAME {
        let head = &out[..MAX_NAME];
        out = match head.rfind('_') {
            Some(cut) if cut >= MAX_NAME / 2 => head[..cut].to_string(),
            _ => head.to_string(),
        };
    }
    let out = out.trim_end_matches('_').to_string();
    // Names are referenced from expressions, where a leading digit is not a
    // name at all.
    if out.is_empty() || !out.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    Some(out)
}

/// The middle of the network view, in world space.
fn default_pos(app: &OtdApp) -> egui::Vec2 {
    if app.canvas_rect.is_positive() {
        app.view
            .to_world(app.canvas_rect.min, app.canvas_rect.center())
            - egui::vec2(crate::canvas::NODE_W, crate::canvas::NODE_H) * 0.5
    } else {
        egui::Vec2::ZERO
    }
}

/// Nudge downward until the spot is not already occupied. Two nodes exactly on
/// top of each other look like one node, and the one underneath is then very
/// hard to find.
fn free_spot(app: &OtdApp, mut pos: egui::Vec2) -> egui::Vec2 {
    let occupied = |app: &OtdApp, pos: egui::Vec2| {
        app.graph.children(app.current).iter().any(|id| {
            let p = app.graph.node(*id).pos;
            (p[0] - pos.x).abs() < crate::canvas::NODE_W * 0.5
                && (p[1] - pos.y).abs() < crate::canvas::NODE_H * 0.5
        })
    };
    let mut guard = 0;
    while occupied(app, pos) && guard < 64 {
        pos.y += crate::canvas::NODE_H + 28.0;
        guard += 1;
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_choose_the_operator() {
        assert_eq!(classify(Path::new("a/b/clip.MP4")), DropKind::Movie);
        assert_eq!(classify(Path::new("still.png")), DropKind::Image);
        assert_eq!(classify(Path::new("loop.wav")), DropKind::Audio);
        assert_eq!(classify(Path::new("show.otd")), DropKind::Project);
        assert_eq!(classify(Path::new("thing.otdc")), DropKind::Component);
        assert_eq!(classify(Path::new("warp.fs")), DropKind::Isf);
        assert_eq!(classify(Path::new("warp.wgsl")), DropKind::Shader("wgsl"));
        assert_eq!(classify(Path::new("cues.csv")), DropKind::Table("comma"));
        assert_eq!(classify(Path::new("notes.md")), DropKind::Text);
        assert_eq!(classify(Path::new("Inter.ttf")), DropKind::Font);
        assert_eq!(classify(Path::new("archive.zip")), DropKind::Unknown);
        assert_eq!(classify(Path::new("no-extension")), DropKind::Unknown);
    }

    #[test]
    fn a_file_name_becomes_a_legal_operator_name() {
        assert_eq!(
            sanitise("My Clip (final)").as_deref(),
            Some("my_clip_final")
        );
        assert_eq!(sanitise("beach").as_deref(), Some("beach"));
        // A name that would not parse as one is better skipped than mangled:
        // the auto-numbered `movie1` is always valid.
        assert_eq!(sanitise("2024-06-11"), None);
        assert_eq!(sanitise("____"), None);
    }

    #[test]
    fn a_long_file_name_is_cut_to_something_a_node_header_can_show() {
        // The name an image generator gives you, which overflowed the node.
        assert_eq!(
            sanitise("chatgpt_image_aug_5_2026_11_59_18_pm").as_deref(),
            Some("chatgpt_image_aug_5")
        );
        // A real one, off a phone: spaces, an emoji and two hashtags, and
        // 29 characters of name once they are gone.
        assert_eq!(
            sanitise("The follower 🧿 #touchdesigner #IK").as_deref(),
            Some("the_follower")
        );
        // No boundary near the end to cut at: a hard cut beats no name.
        assert_eq!(
            sanitise("averyveryverylongsinglewordname").as_deref(),
            Some("averyveryverylongsin")
        );
        for name in [
            "chatgpt_image_aug_5_2026_11_59_18_pm",
            "averyveryverylongsinglewordname",
        ] {
            assert!(sanitise(name).unwrap().len() <= MAX_NAME);
        }
    }

    /// The drop handler sets parameters by name on operators it names. Both
    /// are strings, and a rename on either side would otherwise turn a drop
    /// into a node that quietly plays nothing.
    #[test]
    fn every_operator_a_drop_targets_takes_the_parameters_it_is_given() {
        let registry = otd_engine::registry();
        let targets: &[(&str, &[&str])] = &[
            (otd_gpu::ops::MOVIE_IN, &["file"]),
            (otd_gpu::ops::VIDEO_DEVICE_IN, &[]),
            (otd_gpu::ops::GLSL, &["language", "source"]),
            (otd_gpu::ops::TEXT, &["font", "text"]),
            (otd_chop::io::AUDIO_FILE, &["file"]),
            (otd_dat::ops::TABLE, &["text", "delimiter"]),
            (otd_dat::ops::TEXT, &["text"]),
        ];
        for (type_name, keys) in targets {
            let def = registry
                .get(type_name)
                .unwrap_or_else(|| panic!("{type_name} is not registered"));
            let params = (def.params)();
            for key in *keys {
                assert!(
                    params.contains_key(*key),
                    "{type_name} has no `{key}` parameter for a dropped file to fill"
                );
            }
        }
    }

    #[test]
    fn every_effect_in_the_menu_exists() {
        let registry = otd_engine::registry();
        for type_name in EFFECTS {
            assert!(
                registry.get(type_name).is_some(),
                "{type_name} is offered in the effects menu but is not registered"
            );
        }
    }
}
