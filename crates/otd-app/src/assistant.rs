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
    pub open: bool,
    pub provider: Provider,
    pub model: String,
    /// What is in the key field right now. Saved on demand, never read back
    /// out of storage into the UI — a key you can re-read is a key you can
    /// screenshot.
    pub key_input: String,
    pub prompt: String,
    keys: Keys,
    pending: Option<Receiver<Result<String, String>>>,
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
}

/// A handful of prompts that produce something worth looking at, for the
/// blank-page problem. A chat box with no examples is a chat box nobody uses.
const SUGGESTIONS: &[&str] = &[
    "A slow feedback tunnel in deep blue and gold",
    "Concentric rings pulsing outward from the centre",
    "A kaleidoscope over drifting noise, with trails",
    "Something that reacts to the microphone",
];

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
            let result = otd_ai::provider::complete(&request, &key, &keys);
            let _ = tx.send(result);
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

    let text = match reply {
        Ok(text) => text,
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
