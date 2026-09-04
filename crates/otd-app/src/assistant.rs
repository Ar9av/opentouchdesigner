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

use std::sync::mpsc::{Receiver, TryRecvError};

use egui::{Color32, RichText};
use otd_ai::{Ask, Keys, Provider, patch};

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
    keys: Keys,
    pending: Option<Receiver<Result<(String, bool), String>>>,
    /// Notes from the last plan that was applied.
    pub last: Option<String>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
    pub status: String,
}

impl Default for Assistant {
    fn default() -> Self {
        let keys = Keys::load();
        // Open on a provider that already has a key, so somebody with
        // OPENAI_API_KEY set lands somewhere that works.
        let provider = Provider::ALL
            .iter()
            .copied()
            .find(|p| keys.get(*p).is_some())
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
            keys,
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

    /// Which providers are ready to use, for the picker.
    fn has_key(&self, provider: Provider) -> bool {
        self.keys.get(provider).is_some()
    }

    /// Nothing configured for the provider currently selected.
    fn keys_missing(&self) -> bool {
        !self.has_key(self.provider) && self.key_input.trim().is_empty()
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
                });
        });
}

fn bar_contents(app: &mut OtdApp, ui: &mut egui::Ui) {
    let busy = app.assistant.busy();

    // ---- the prompt line
    let edit = ui.add(
        egui::TextEdit::multiline(&mut app.assistant.prompt)
            .frame(egui::Frame::NONE)
            .desired_rows(1)
            .desired_width(f32::INFINITY)
            .hint_text("Describe a patch, and it gets built here…"),
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

        // The model chip, as in every chat UI: provider and model in one
        // place, because they are one decision.
        let provider = app.assistant.provider;
        let chip = format!("✨ {}", short_model(&app.assistant.model));
        egui::ComboBox::from_id_salt("assistant-bar-model")
            .selected_text(chip)
            .show_ui(ui, |ui| {
                for p in Provider::ALL {
                    let has = app.assistant.has_key(*p);
                    ui.label(
                        RichText::new(format!("{} {}", if has { "●" } else { "○" }, p.label()))
                            .small()
                            .weak(),
                    );
                    for model in p.models() {
                        let selected = provider == *p && app.assistant.model == *model;
                        if ui.selectable_label(selected, *model).clicked() {
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

        if app.assistant.keys_missing() {
            ui.label(
                RichText::new("no key")
                    .small()
                    .color(Color32::from_rgb(235, 170, 90)),
            )
            .on_hover_text("Click ⚙ to paste an API key");
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
                let ready = !app.assistant.prompt.trim().is_empty();
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
                    let mark = if app.assistant.has_key(*provider) {
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
        ui.label(RichText::new("● has a key").weak().small());
    });

    ui.horizontal(|ui| {
        ui.label("Model");
        let provider = app.assistant.provider;
        egui::ComboBox::from_id_salt("ai-model")
            .selected_text(app.assistant.model.clone())
            .show_ui(ui, |ui| {
                for model in provider.models() {
                    if ui
                        .selectable_label(app.assistant.model == *model, *model)
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

    // ---- key
    let provider = app.assistant.provider;
    ui.horizontal(|ui| {
        ui.label("API key");
        ui.add(
            egui::TextEdit::singleline(&mut app.assistant.key_input)
                .password(true)
                .desired_width(220.0)
                .hint_text(provider.env_var()),
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
                    provider.env_var()
                ))
                .weak()
                .small(),
            ),
        };
        ui.hyperlink_to(RichText::new("get one").small(), provider.console_url());
    });

    ui.separator();

    // ---- the prompt
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
            .hint_text("Describe what you want to see…"),
    );

    ui.horizontal_wrapped(|ui| {
        for suggestion in SUGGESTIONS {
            if ui.small_button(*suggestion).clicked() {
                app.assistant.prompt = suggestion.to_string();
            }
        }
    });

    ui.horizontal(|ui| {
        let ready = !app.assistant.busy() && !app.assistant.prompt.trim().is_empty();
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
    let Some(key) = keys.get(app.assistant.provider).cloned() else {
        app.assistant.error = Some(format!(
            "no API key for {} — paste one, or set {}",
            app.assistant.provider.label(),
            app.assistant.provider.env_var()
        ));
        return;
    };

    let request = otd_ai::request_for(&Ask {
        provider: app.assistant.provider,
        model: app.assistant.model.clone(),
        prompt: app.assistant.prompt.clone(),
        graph: &app.graph,
        parent: app.current,
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
            if repaired {
                app.assistant.status.push_str(" · shader fixed on retry");
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
    fn it_opens_on_a_provider_that_has_a_key() {
        // The default has to be useful on a machine where exactly one of the
        // three is configured, which is the normal case.
        let assistant = Assistant::default();
        if Provider::ALL.iter().any(|p| assistant.has_key(*p)) {
            assert!(
                assistant.has_key(assistant.provider),
                "opened on {:?} with no key",
                assistant.provider
            );
        }
        assert!(!assistant.model.is_empty());
        assert!(!assistant.busy());
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
