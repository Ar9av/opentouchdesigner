//! The assistant panel: describe a patch, get operators.
//!
//! Three things this panel is careful about.
//!
//! **The request happens on a worker thread.** A completion takes anywhere
//! from two seconds to a minute, and a frozen editor for a minute is not a
//! realtime tool. The prompt is built here, where the graph is; the HTTP call
//! happens there; the reply is validated and applied back here.
//!
//! **The result is one undo.** A checkpoint is taken before anything is
//! created, so a patch you did not want is `Cmd+Z` and gone, not nine
//! deletions.
//!
//! **The key field is not where the key lives.** It is typed here and saved
//! to a `0600` file outside every project, or it comes from the environment
//! and is never typed at all. It does not go into the `.otd`.
//!
//! Two of the five providers have no key at all: Claude Code and Codex run
//! the CLI this machine is already signed in to, so "configured" means "the
//! binary is there" rather than "a key is stored". [`Provider::needs_key`]
//! decides which panel a provider gets, and [`Assistant::ready`] is the one
//! question the picker asks either way.

use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, TryRecvError};

use egui::{Color32, RichText};
use otd_ai::{Ask, Keys, Provider, cli, patch};

use crate::app::OtdApp;

pub struct Assistant {
    /// The settings window: providers, keys, what was skipped.
    pub open: bool,
    /// The floating bar over the canvas. On by default — a feature behind a
    /// menu is a feature nobody finds — and dismissable, because a bar over
    /// your work that you cannot get rid of is worse than no bar.
    pub bar: bool,
    /// Collapsed to a pill. Out of the way without being gone.
    pub collapsed: bool,
    /// Set for one frame to move the caret into the bar.
    focus_bar: bool,
    pub provider: Provider,
    pub model: String,
    /// What is in the key field right now. Saved on demand, never read back
    /// out of storage into the UI — a key you can re-read is a key you can
    /// screenshot.
    pub key_input: String,
    pub prompt: String,
    /// A still to work back from. With one attached the prompt is optional:
    /// pointing at a picture is a complete request.
    pub image: Option<otd_ai::Image>,
    /// The thumbnail, uploaded once and dropped whenever the image changes.
    /// Rebuilding a texture every frame is how a 96-pixel preview becomes a
    /// performance problem.
    image_tex: Option<egui::TextureHandle>,
    /// Where the bar was last drawn, so a file dropped on it is attached
    /// rather than turned into a Movie File In. See [`claim_drop`].
    pub bar_rect: egui::Rect,
    keys: Keys,
    /// Whether each CLI provider's binary was found. A filesystem lookup, so
    /// cheap enough to do at startup — unlike asking it its version, which
    /// spawns a process and waits for it.
    cli_found: BTreeMap<Provider, bool>,
    /// What `--version` said, filled in the first time the settings window
    /// draws that provider and on demand after that.
    cli_version: BTreeMap<Provider, Result<String, String>>,
    pending: Option<Receiver<Result<(String, Option<otd_ai::Repair>), String>>>,
    /// Notes from the last plan that was applied.
    pub last: Option<String>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
    pub status: String,
}

impl Default for Assistant {
    fn default() -> Self {
        let keys = Keys::load();
        let cli_found: BTreeMap<Provider, bool> = Provider::ALL
            .iter()
            .filter(|p| !p.needs_key())
            .map(|p| (*p, cli::binary(*p).is_some()))
            .collect();
        // Open on a provider that is already usable, so somebody with
        // OPENAI_API_KEY set — or with Claude Code installed and nothing
        // else — lands somewhere that works.
        let provider = Provider::ALL
            .iter()
            .copied()
            .find(|p| match p.needs_key() {
                true => keys.get(*p).is_some(),
                false => cli_found.get(p).copied().unwrap_or(false),
            })
            .unwrap_or(Provider::Anthropic);
        Assistant {
            open: false,
            bar: true,
            collapsed: false,
            focus_bar: false,
            provider,
            model: provider.default_model().to_string(),
            key_input: String::new(),
            prompt: String::new(),
            image: None,
            image_tex: None,
            bar_rect: egui::Rect::NOTHING,
            keys,
            cli_found,
            cli_version: BTreeMap::new(),
            pending: None,
            last: None,
            warnings: Vec::new(),
            error: None,
            status: String::new(),
        }
    }
}

impl Assistant {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Which providers are ready to use, for the picker. A key for the three
    /// that take one, an installed CLI for the two that do not.
    fn ready(&self, provider: Provider) -> bool {
        match provider.needs_key() {
            true => self.keys.get(provider).is_some(),
            false => self.cli_found.get(&provider).copied().unwrap_or(false),
        }
    }

    /// Nothing configured for the provider currently selected. A key typed
    /// but not yet saved counts — it works for this request.
    fn not_configured(&self) -> bool {
        !self.ready(self.provider)
            && (!self.provider.needs_key() || self.key_input.trim().is_empty())
    }

    /// Re-run the CLI lookup, for after somebody installs one without
    /// restarting the editor.
    fn recheck_cli(&mut self, provider: Provider) {
        self.cli_found
            .insert(provider, cli::binary(provider).is_some());
        self.cli_version.insert(provider, cli::detect(provider));
    }

    /// There is enough here to ask with: a prompt, a picture, or both.
    ///
    /// An image on its own is a request — "make this" — which is why this is
    /// not simply a check on the text box.
    pub fn has_request(&self) -> bool {
        !self.prompt.trim().is_empty() || self.image.is_some()
    }

    /// Load a file as the reference image, reporting failure where the user
    /// is already looking rather than swallowing it.
    ///
    /// Decoding and shrinking a large still takes a moment; it happens here,
    /// once, rather than on the worker at send time, so the size shown in the
    /// bar is the size that will actually be sent.
    pub fn attach(&mut self, path: &std::path::Path) {
        match otd_ai::Image::load(path) {
            Ok(image) => {
                self.image = Some(image);
                self.image_tex = None;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    pub fn detach(&mut self) {
        self.image = None;
        self.image_tex = None;
    }

    /// The thumbnail as a texture, uploaded on first use.
    fn thumbnail(&mut self, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        let image = self.image.as_ref()?;
        if self.image_tex.is_none() {
            let thumb = &image.thumb;
            let pixels = egui::ColorImage::from_rgba_unmultiplied(
                [thumb.width as usize, thumb.height as usize],
                &thumb.rgba,
            );
            self.image_tex =
                Some(ctx.load_texture("assistant-reference", pixels, egui::TextureOptions::LINEAR));
        }
        self.image_tex.clone()
    }
}

/// The extensions the attach dialog offers. Deliberately the decoders this
/// build actually has rather than every format with a magic number.
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "bmp", "tga", "tif", "tiff", "ico", "ppm", "pgm", "pnm",
];

fn looks_like_an_image(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.iter().any(|k| e.eq_ignore_ascii_case(k)))
        .unwrap_or(false)
}

/// Whether a drop at this position would be taken as the reference image.
///
/// Split out from [`claim_drop`] so the hover overlay can promise exactly
/// what the drop will do. An overlay that says "Movie File In" over a bar
/// that is about to attach a reference is worse than no overlay.
pub fn would_claim(app: &OtdApp, paths: &[std::path::PathBuf], at: Option<egui::Pos2>) -> bool {
    if !app.assistant.bar || app.assistant.collapsed || app.perform {
        return false;
    }
    let Some(at) = at else { return false };
    // A drop on the bar that is not an image is not ours; the media router
    // should have it rather than us silently eating it.
    app.assistant.bar_rect.contains(at) && paths.iter().any(|p| looks_like_an_image(p))
}

/// Take a dropped file as the reference image, if it is one and it landed on
/// the bar.
///
/// Called before the media router, and returning `true` means "handled" —
/// otherwise the same drop would also become a Movie File In on the canvas,
/// which is the correct thing to do with an image dropped anywhere *else*.
pub fn claim_drop(app: &mut OtdApp, paths: &[std::path::PathBuf], at: Option<egui::Pos2>) -> bool {
    if !would_claim(app, paths, at) {
        return false;
    }
    let Some(image) = paths.iter().find(|p| looks_like_an_image(p)).cloned() else {
        return false;
    };
    app.assistant.attach(&image);
    true
}

/// The attach button and the chip that replaces it once something is on.
///
/// Shared by the bar and the settings window, because there is one attachment
/// and it should look the same wherever it is shown.
fn attachment_row(app: &mut OtdApp, ui: &mut egui::Ui) {
    let texture = app.assistant.thumbnail(ui.ctx());
    let Some(image) = &app.assistant.image else {
        if ui
            .small_button("📎")
            .on_hover_text("Attach a reference image to work back from — or drop one on the bar")
            .clicked()
        {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Images", IMAGE_EXTENSIONS)
                .pick_file()
            {
                app.assistant.attach(&path);
            }
        }
        return;
    };

    let name = image.name();
    let detail = image.detail();
    if let Some(texture) = texture {
        ui.add(
            egui::Image::new(&texture)
                .max_height(20.0)
                .corner_radius(3.0),
        )
        .on_hover_text(format!("{name} — {detail}"));
    }
    ui.label(RichText::new(short_name(&name)).small().weak())
        .on_hover_text(format!(
            "{name} — {detail}\nThe patch is built to match this."
        ));
    if ui
        .small_button("×")
        .on_hover_text("Remove the reference image")
        .clicked()
    {
        app.assistant.detach();
    }
}

/// A filename that fits in a bar that is also holding a model picker.
fn short_name(name: &str) -> String {
    if name.chars().count() <= 18 {
        return name.to_string();
    }
    let head: String = name.chars().take(10).collect();
    let tail: String = name
        .chars()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}…{tail}")
}

/// What to show for a model name, given that one of them is deliberately
/// empty. See `Provider::models` for why Codex has no default.
fn model_label(model: &str) -> &str {
    match model.trim().is_empty() {
        true => "(CLI default)",
        false => model,
    }
}

/// A handful of prompts that produce something worth looking at, for the
/// blank-page problem. A chat box with no examples is a chat box nobody uses.
const SUGGESTIONS: &[&str] = &[
    "A slow feedback tunnel in deep blue and gold",
    "Concentric rings pulsing outward from the centre",
    "A kaleidoscope over drifting noise, with trails",
    "Something that reacts to the microphone",
];

/// The floating bar over the canvas.
///
/// Deliberately not a docked panel: it hovers, it takes no layout space, and
/// it goes away. `Cmd/Ctrl+K` shows it and puts the caret in it from
/// anywhere; `Escape` collapses it to a pill; the pill clicks back open. It
/// is hidden entirely in perform mode, where the window is the show.
pub fn bar(app: &mut OtdApp, ctx: &egui::Context) {
    if app.perform {
        return;
    }

    // Cmd/Ctrl+K from anywhere. The one shortcut worth spending, because a
    // prompt box you have to go and find is a prompt box you do not use.
    let summon = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::K));
    if summon {
        app.assistant.bar = true;
        app.assistant.collapsed = false;
        app.assistant.focus_bar = true;
    }
    if !app.assistant.bar {
        return;
    }

    poll(app);

    if app.assistant.collapsed {
        collapsed_pill(app, ctx);
        return;
    }

    egui::Area::new(egui::Id::new("assistant-bar"))
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -56.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            // The window frame from the active theme, so the bar looks like
            // part of the program rather than pasted onto it.
            egui::Frame::window(ui.style())
                .corner_radius(egui::CornerRadius::same(14))
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_width(560.0);
                    bar_contents(app, ui);
                    // Remembered so a file dropped here is attached rather
                    // than built into the network. See `claim_drop`.
                    app.assistant.bar_rect = ui.min_rect().expand(12.0);
                });
        });
}

fn bar_contents(app: &mut OtdApp, ui: &mut egui::Ui) {
    let busy = app.assistant.busy();

    // ---- the prompt line
    //
    // The hint changes when a picture is attached, because what the box is
    // for changes: with a reference, typing nothing is a complete request and
    // whatever you do type is a correction to it.
    let hint = match app.assistant.image.is_some() {
        true => "Enter to build this image — or say what to change about it…",
        false => "Describe a patch, and it gets built here…",
    };
    let edit = ui.add(
        egui::TextEdit::multiline(&mut app.assistant.prompt)
            .frame(egui::Frame::NONE)
            .desired_rows(1)
            .desired_width(f32::INFINITY)
            .hint_text(hint),
    );
    if std::mem::take(&mut app.assistant.focus_bar) {
        edit.request_focus();
    }
    // Enter sends, Shift+Enter is a newline — the convention every chat box
    // has trained everyone into.
    let entered =
        edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
    if entered {
        // The newline arrives with the key press; take it back out.
        let trimmed = app.assistant.prompt.trim_end_matches('\n').to_string();
        app.assistant.prompt = trimmed;
    }

    ui.add_space(4.0);

    // ---- the control row
    ui.horizontal(|ui| {
        if ui
            .small_button("⚙")
            .on_hover_text("Providers, API keys and what was skipped")
            .clicked()
        {
            app.assistant.open = true;
        }

        attachment_row(app, ui);

        // The model chip, as in every chat UI: provider and model in one
        // place, because they are one decision.
        let provider = app.assistant.provider;
        let chip = format!("✨ {}", short_model(&app.assistant.model));
        egui::ComboBox::from_id_salt("assistant-bar-model")
            .selected_text(chip)
            .show_ui(ui, |ui| {
                for p in Provider::ALL {
                    let has = app.assistant.ready(*p);
                    ui.label(
                        RichText::new(format!("{} {}", if has { "●" } else { "○" }, p.label()))
                            .small()
                            .weak(),
                    );
                    for model in p.models() {
                        let selected = provider == *p && app.assistant.model == *model;
                        if ui.selectable_label(selected, model_label(model)).clicked() {
                            app.assistant.provider = *p;
                            app.assistant.model = model.to_string();
                        }
                    }
                    ui.separator();
                }
                if ui.button("More settings…").clicked() {
                    app.assistant.open = true;
                }
            });

        if app.assistant.not_configured() {
            let (short, hint) = match app.assistant.provider.needs_key() {
                true => ("no key", "Click ⚙ to paste an API key"),
                false => (
                    "not installed",
                    "Click ⚙ — this provider runs a CLI that is not on this machine",
                ),
            };
            ui.label(
                RichText::new(short)
                    .small()
                    .color(Color32::from_rgb(235, 170, 90)),
            )
            .on_hover_text(hint);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("×")
                .on_hover_text("Hide the bar (Cmd/Ctrl+K brings it back)")
                .clicked()
            {
                app.assistant.bar = false;
            }
            if ui
                .small_button("–")
                .on_hover_text("Collapse (Escape)")
                .clicked()
            {
                app.assistant.collapsed = true;
            }
            if busy {
                ui.spinner();
            } else {
                let ready = app.assistant.has_request();
                if ui
                    .add_enabled(ready, egui::Button::new("▶"))
                    .on_hover_text("Build it (Enter)")
                    .clicked()
                    || (entered && ready)
                {
                    send(app);
                }
            }
        });
    });

    // ---- one line of what happened, so the bar answers on its own
    if let Some(error) = &app.assistant.error {
        ui.label(
            RichText::new(one_line(error))
                .small()
                .color(Color32::from_rgb(235, 120, 120)),
        )
        .on_hover_text(error);
    } else if let Some(notes) = &app.assistant.last {
        ui.horizontal(|ui| {
            ui.label(RichText::new(one_line(notes)).small().weak())
                .on_hover_text(notes);
            if !app.assistant.warnings.is_empty() && ui.small_button("⚠").clicked() {
                app.assistant.open = true;
            }
        });
    }

    // Escape collapses rather than closing: the work you typed survives.
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) && !app.assistant.prompt.is_empty() {
        app.assistant.collapsed = true;
    }
}

fn collapsed_pill(app: &mut OtdApp, ctx: &egui::Context) {
    egui::Area::new(egui::Id::new("assistant-pill"))
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -56.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::window(ui.style())
                .corner_radius(egui::CornerRadius::same(14))
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if app.assistant.busy() {
                            ui.spinner();
                        }
                        if ui
                            .button("✨ Assistant")
                            .on_hover_text("Cmd/Ctrl+K")
                            .clicked()
                        {
                            app.assistant.collapsed = false;
                            app.assistant.focus_bar = true;
                        }
                    });
                });
        });
}

/// `anthropic/claude-sonnet-4.5` is too long for a chip; the last segment
/// with the date suffix dropped is what identifies it to a human.
fn short_model(model: &str) -> String {
    let tail = model.rsplit('/').next().unwrap_or(model);
    let trimmed = match tail.rsplit_once('-') {
        // Drop a trailing 8-digit date, keep everything else.
        Some((head, last)) if last.len() == 8 && last.chars().all(|c| c.is_ascii_digit()) => head,
        _ => tail,
    };
    if trimmed.chars().count() > 22 {
        format!("{}…", trimmed.chars().take(21).collect::<String>())
    } else {
        trimmed.to_string()
    }
}

/// The first sentence, for a one-line status.
fn one_line(text: &str) -> String {
    let first = text.split(['\n', '.']).next().unwrap_or(text).trim();
    if first.chars().count() > 90 {
        format!("{}…", first.chars().take(89).collect::<String>())
    } else {
        first.to_string()
    }
}

pub fn window(app: &mut OtdApp, ctx: &egui::Context) {
    if !app.assistant.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Assistant")
        .open(&mut open)
        .default_width(420.0)
        .resizable(true)
        .show(ctx, |ui| body(app, ui));
    if !open {
        app.assistant.open = false;
    }
    // A request in flight has to keep the UI repainting, or the reply lands
    // in a window that is not being drawn.
    if app.assistant.busy() {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

fn body(app: &mut OtdApp, ui: &mut egui::Ui) {
    poll(app);

    // ---- provider and model
    ui.horizontal(|ui| {
        ui.label("Provider");
        let current = app.assistant.provider;
        egui::ComboBox::from_id_salt("ai-provider")
            .selected_text(current.label())
            .show_ui(ui, |ui| {
                for provider in Provider::ALL {
                    let mark = if app.assistant.ready(*provider) {
                        "●"
                    } else {
                        "○"
                    };
                    if ui
                        .selectable_label(
                            current == *provider,
                            format!("{mark} {}", provider.label()),
                        )
                        .clicked()
                        && current != *provider
                    {
                        app.assistant.provider = *provider;
                        app.assistant.model = provider.default_model().to_string();
                        app.assistant.key_input.clear();
                        app.assistant.error = None;
                    }
                }
            });
        ui.label(RichText::new("● ready to use").weak().small());
    });

    ui.horizontal(|ui| {
        ui.label("Model");
        let provider = app.assistant.provider;
        egui::ComboBox::from_id_salt("ai-model")
            .selected_text(model_label(&app.assistant.model).to_string())
            .show_ui(ui, |ui| {
                for model in provider.models() {
                    if ui
                        .selectable_label(app.assistant.model == *model, model_label(model))
                        .clicked()
                    {
                        app.assistant.model = model.to_string();
                    }
                }
            });
        // Free text as well as the list: the list will be out of date before
        // anybody reads it, and a model name is not a closed set.
        ui.add(
            egui::TextEdit::singleline(&mut app.assistant.model)
                .desired_width(f32::INFINITY)
                .hint_text("or type a model name"),
        );
    });

    // ---- key, or the CLI that stands in for one
    let provider = app.assistant.provider;
    if !provider.needs_key() {
        cli_status(app, ui, provider);
        ui.separator();
        prompt_section(app, ui);
        return;
    }
    ui.horizontal(|ui| {
        ui.label("API key");
        ui.add(
            egui::TextEdit::singleline(&mut app.assistant.key_input)
                .password(true)
                .desired_width(220.0)
                .hint_text(provider.env_var().unwrap_or_default()),
        );
        if ui
            .add_enabled(
                !app.assistant.key_input.trim().is_empty(),
                egui::Button::new("Save"),
            )
            .on_hover_text("Store this key for next time, outside every project")
            .clicked()
        {
            let key = otd_ai::Key::new(app.assistant.key_input.clone());
            app.assistant.keys.set(provider, key);
            match app.assistant.keys.save() {
                Ok(path) => {
                    app.assistant.key_input.clear();
                    app.assistant.status = format!("Key saved to {}", path.display());
                    app.assistant.error = None;
                }
                Err(e) => app.assistant.error = Some(e),
            }
        }
    });
    ui.horizontal(|ui| {
        match app.assistant.keys.get(provider) {
            Some(key) => ui.label(
                RichText::new(format!("stored: {}", key.hint()))
                    .weak()
                    .small(),
            ),
            None => ui.label(
                RichText::new(format!(
                    "no key — paste one, or set {} in the environment",
                    provider.env_var().unwrap_or_default()
                ))
                .weak()
                .small(),
            ),
        };
        ui.hyperlink_to(RichText::new("get one").small(), provider.console_url());
    });

    ui.separator();
    prompt_section(app, ui);
}

/// What stands in for the key field on a provider that has no key: whether
/// the CLI is there, which one, and how to fix it if it is not.
///
/// The version check spawns a process, so it happens once — the first time
/// this draws for a provider — rather than every frame.
fn cli_status(app: &mut OtdApp, ui: &mut egui::Ui, provider: Provider) {
    if !app.assistant.cli_version.contains_key(&provider) {
        app.assistant.recheck_cli(provider);
    }

    ui.horizontal(|ui| {
        ui.label("Signed in via");
        match app.assistant.cli_version.get(&provider) {
            Some(Ok(version)) => {
                ui.label(RichText::new(version).monospace().small());
            }
            Some(Err(e)) => {
                ui.label(
                    RichText::new(e)
                        .small()
                        .color(Color32::from_rgb(235, 170, 90)),
                );
            }
            None => {
                ui.label(RichText::new("checking…").weak().small());
            }
        }
        if ui
            .small_button("Re-check")
            .on_hover_text("Look for the CLI again — after installing it, say")
            .clicked()
        {
            app.assistant.recheck_cli(provider);
        }
    });
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(
                "Uses this machine's own login, so it costs subscription quota rather than \
                 API credit. Nothing is billed per token and no key is stored.",
            )
            .weak()
            .small(),
        );
    });
    ui.hyperlink_to(
        RichText::new("install and sign in").small(),
        provider.console_url(),
    );
}

/// The prompt box and everything under it. Shared by both halves of the
/// settings window, which differ only in what sits above it.
fn prompt_section(app: &mut OtdApp, ui: &mut egui::Ui) {
    ui.label(
        RichText::new(format!(
            "Builds into {} — the network you are looking at.",
            app.graph.path(app.current)
        ))
        .weak()
        .small(),
    );
    ui.add(
        egui::TextEdit::multiline(&mut app.assistant.prompt)
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            .hint_text(match app.assistant.image.is_some() {
                true => "What to change about the reference — or nothing, to just build it…",
                false => "Describe what you want to see…",
            }),
    );

    ui.horizontal(|ui| {
        ui.label(RichText::new("Reference").weak().small());
        attachment_row(app, ui);
        if app.assistant.image.is_none() {
            ui.label(
                RichText::new("a still to work back from — the look is rebuilt as operators")
                    .weak()
                    .small(),
            );
        }
    });

    ui.horizontal_wrapped(|ui| {
        for suggestion in SUGGESTIONS {
            if ui.small_button(*suggestion).clicked() {
                app.assistant.prompt = suggestion.to_string();
            }
        }
    });

    ui.horizontal(|ui| {
        let ready = !app.assistant.busy() && app.assistant.has_request();
        if ui
            .add_enabled(ready, egui::Button::new("Build it"))
            .clicked()
        {
            send(app);
        }
        if app.assistant.busy() {
            ui.spinner();
            ui.label(RichText::new("thinking…").weak());
        }
        if !app.assistant.status.is_empty() {
            ui.label(RichText::new(&app.assistant.status).weak().small());
        }
    });

    // ---- what happened
    if let Some(error) = &app.assistant.error {
        ui.colored_label(Color32::from_rgb(235, 120, 120), error);
    }
    if let Some(notes) = &app.assistant.last {
        ui.separator();
        ui.label(notes);
    }
    if !app.assistant.warnings.is_empty() {
        ui.collapsing(
            format!("{} thing(s) skipped", app.assistant.warnings.len()),
            |ui| {
                for warning in &app.assistant.warnings {
                    ui.label(RichText::new(warning).small().monospace());
                }
            },
        );
    }
    if app.assistant.last.is_some() {
        ui.label(
            RichText::new("Not what you wanted? Cmd/Ctrl+Z undoes the whole thing.")
                .weak()
                .small(),
        );
    }
}

/// The shader compiler, for the worker to check the model's work with.
///
/// Naga's front end — the same one that would reject the shader a moment
/// later on the node, so a shader that passes here compiles there.
fn check_shader(source: &str, is_glsl: bool) -> Result<(), String> {
    if is_glsl {
        otd_gpu::shader::validate_glsl(&otd_gpu::shader::wrap_glsl(source))
    } else {
        otd_gpu::shader::validate_wgsl(&otd_gpu::shader::wrap_wgsl(source))
    }
}

/// Build the request here, where the graph is, and hand it to a worker.
fn send(app: &mut OtdApp) {
    app.assistant.error = None;
    app.assistant.warnings.clear();
    app.assistant.last = None;
    app.assistant.status.clear();

    // A key typed but not saved should still work for this one request.
    let mut keys = app.assistant.keys.clone();
    if !app.assistant.key_input.trim().is_empty() {
        keys.set(
            app.assistant.provider,
            otd_ai::Key::new(app.assistant.key_input.clone()),
        );
    }
    // A CLI provider has no key to be missing; `cli::complete` reports an
    // absent or signed-out binary itself, in its own words.
    let key = match app.assistant.provider.env_var() {
        Some(var) => match keys.get(app.assistant.provider).cloned() {
            Some(key) => key,
            None => {
                app.assistant.error = Some(format!(
                    "no API key for {} — paste one, or set {var}",
                    app.assistant.provider.label(),
                ));
                return;
            }
        },
        None => otd_ai::Key::new(""),
    };

    let request = otd_ai::request_for(&Ask {
        provider: app.assistant.provider,
        model: app.assistant.model.clone(),
        prompt: app.assistant.prompt.clone(),
        image: app.assistant.image.clone(),
        graph: &app.graph,
        parent: app.current,
        selected: app.selected,
        registry: &app.registry,
    });

    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("otd-assistant".into())
        .spawn(move || {
            let result = otd_ai::complete_with_repair(&request, &key, &keys, Some(check_shader));
            let _ = tx.send(result.map(|r| (r.text, r.repaired)));
        });
    app.assistant.pending = Some(rx);
}

/// Take the reply if it has arrived, and build it.
fn poll(app: &mut OtdApp) {
    let Some(rx) = &app.assistant.pending else {
        return;
    };
    let reply = match rx.try_recv() {
        Ok(reply) => reply,
        Err(TryRecvError::Empty) => return,
        Err(TryRecvError::Disconnected) => {
            app.assistant.pending = None;
            app.assistant.error = Some("the request thread stopped unexpectedly".into());
            return;
        }
    };
    app.assistant.pending = None;

    let (text, repaired) = match reply {
        Ok(pair) => pair,
        Err(e) => {
            app.assistant.error = Some(e);
            return;
        }
    };
    let plan = match otd_ai::plan_from_reply(&text, &app.registry) {
        Ok(plan) => plan,
        Err(e) => {
            app.assistant.error = Some(e);
            return;
        }
    };

    // One checkpoint, before anything is created, so the whole patch undoes
    // as the single thing it was.
    app.edit("assistant");
    let current = app.current;
    match patch::apply(&mut app.graph, current, &app.registry, &plan) {
        Ok((applied, viewer)) => {
            if let Some(viewer) = viewer {
                app.viewer = Some(viewer);
            }
            app.selected = applied
                .created
                .first()
                .and_then(|name| app.graph.find_from(current, name));
            app.assistant.status = format!(
                "{} node(s), {} wire(s)",
                applied.created.len(),
                applied.wired
            );
            app.assistant.warnings = applied.warnings;
            // Anything the model built and then forgot to join on. The nodes
            // are real and they cook; saying so beats leaving them to be
            // found.
            let loose = patch::dangling(&plan);
            if !loose.is_empty() {
                app.assistant.warnings.push(format!(
                    "created but wired to nothing: {}",
                    loose.join(", ")
                ));
            }
            // A shader that survived the repair round still broken would
            // otherwise be a red node and a silently black output.
            if let Ok(json) = patch::extract_json(&text) {
                for (node, error) in patch::shader_problems(&json, check_shader) {
                    app.assistant
                        .warnings
                        .push(format!("{node}: shader still does not compile — {error}"));
                }
            }
            if let Some(repair) = repaired {
                app.assistant
                    .status
                    .push_str(&format!(" · {}", repair.label()));
            }
            app.assistant.last = Some(if plan.notes.trim().is_empty() {
                "Built.".to_string()
            } else {
                plan.notes.clone()
            });
        }
        Err(e) => app.assistant.error = Some(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_opens_on_a_provider_that_is_usable() {
        // The default has to be useful on a machine where exactly one of the
        // five is configured, which is the normal case — and "configured"
        // means a key for three of them and an installed CLI for two.
        let assistant = Assistant::default();
        if Provider::ALL.iter().any(|p| assistant.ready(*p)) {
            assert!(
                assistant.ready(assistant.provider),
                "opened on {:?}, which is not usable",
                assistant.provider
            );
        }
        assert!(!assistant.busy());
        // Codex's default model is deliberately empty, so `model` may be too;
        // what must hold is that it is one this provider offers.
        assert!(
            assistant
                .provider
                .models()
                .contains(&assistant.model.as_str()),
            "{:?} opened on a model it does not list: {:?}",
            assistant.provider,
            assistant.model
        );
    }

    #[test]
    fn a_cli_provider_never_asks_for_a_key() {
        // The bar says "not installed" rather than "no key", and no amount of
        // typing in a key field it does not have makes it configured.
        let assistant = Assistant {
            provider: Provider::ClaudeCode,
            key_input: "sk-ant-whatever".into(),
            ..Default::default()
        };
        assert_eq!(
            assistant.not_configured(),
            !assistant.ready(Provider::ClaudeCode)
        );
        assert!(Provider::ClaudeCode.env_var().is_none());
    }

    #[test]
    fn a_picture_on_its_own_is_a_complete_request() {
        // The send button is gated on this, and an attached reference with an
        // empty box has to enable it — "build this" needs no words.
        let mut assistant = Assistant::default();
        assert!(!assistant.has_request(), "nothing typed, nothing attached");
        assistant.prompt = "  ".into();
        assert!(!assistant.has_request(), "whitespace is not a request");
        assistant.prompt = "a blue tunnel".into();
        assert!(assistant.has_request());
    }

    #[test]
    fn only_images_are_taken_as_references() {
        // Everything else dropped on the bar belongs to the media router, and
        // eating it silently would lose the drop entirely.
        for name in ["reference.png", "shot.JPG", "grab.webp"] {
            assert!(looks_like_an_image(std::path::Path::new(name)), "{name}");
        }
        for name in ["clip.mov", "patch.otd", "shader.glsl", "notes", "a.png.txt"] {
            assert!(!looks_like_an_image(std::path::Path::new(name)), "{name}");
        }
    }

    #[test]
    fn a_long_filename_is_shortened_from_the_middle() {
        // Both ends carry meaning — the subject at the front, the extension at
        // the back — so it is the middle that goes.
        let short = short_name("ref.png");
        assert_eq!(short, "ref.png");
        let long = short_name("a-very-long-reference-screenshot-name.png");
        assert!(long.chars().count() <= 18, "{long}");
        assert!(long.starts_with("a-very-lon"), "{long}");
        assert!(long.ends_with(".png"), "{long}");
    }

    #[test]
    fn an_empty_model_reads_as_the_clis_own_default() {
        assert_eq!(model_label(""), "(CLI default)");
        assert_eq!(model_label("  "), "(CLI default)");
        assert_eq!(model_label("sonnet"), "sonnet");
    }

    #[test]
    fn the_suggestions_are_prompts_rather_than_labels() {
        // They are pasted into the box verbatim, so each has to read as a
        // request on its own.
        for s in SUGGESTIONS {
            assert!(s.len() > 20, "{s}");
            assert!(!s.ends_with('.'), "{s}");
        }
    }
}

#[cfg(test)]
mod bar_tests {
    use super::*;

    #[test]
    fn a_model_name_is_shortened_to_something_that_fits_a_chip() {
        // The provider prefix and the date suffix are the two parts nobody
        // reads, and together they are most of the string.
        assert_eq!(
            short_model("anthropic/claude-sonnet-4.5"),
            "claude-sonnet-4.5"
        );
        assert_eq!(
            short_model("claude-sonnet-4-5-20250929"),
            "claude-sonnet-4-5"
        );
        assert_eq!(short_model("gpt-5"), "gpt-5");
        // A date-like tail that is not a date is left alone.
        assert_eq!(short_model("llama-4-maverick"), "llama-4-maverick");
        // And anything still absurd is truncated rather than blowing the row.
        assert!(short_model(&"x".repeat(60)).chars().count() <= 22);
    }

    #[test]
    fn a_status_line_is_one_line() {
        let notes = "Built a tunnel.\nTurn decay1.brightness for trail length.";
        assert_eq!(one_line(notes), "Built a tunnel");
        assert!(!one_line(notes).contains('\n'));
        let long = "a ".repeat(200);
        assert!(one_line(&long).chars().count() <= 90);
    }

    #[test]
    fn the_bar_is_on_by_default_and_can_be_got_rid_of() {
        // A feature behind a menu is a feature nobody finds; a bar over your
        // work that you cannot dismiss is worse than no bar.
        let mut a = Assistant::default();
        assert!(a.bar);
        assert!(!a.collapsed);
        a.collapsed = true;
        assert!(a.bar, "collapsing is not hiding");
        a.bar = false;
        assert!(!a.bar);
    }
}
