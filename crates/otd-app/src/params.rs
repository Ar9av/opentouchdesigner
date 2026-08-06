//! The parameter panel.
//!
//! The four-mode system (PLAN.md §2.3) is the thing this panel exists to make
//! visible. Every parameter carries a mode button; clicking it cycles the
//! parameter between a constant and an expression without losing the constant
//! underneath. Export and Bind are shown but disabled until CHOPs land, so the
//! shape of the system is legible from day one.

use egui::{Color32, RichText};
use otd_core::{ParamMode, Value};

use crate::app::OtdApp;

pub fn show(app: &mut OtdApp, ui: &mut egui::Ui) {
    let Some(id) = app.selected.filter(|s| app.graph.contains(*s)) else {
        ui.label(RichText::new("No operator selected").weak());
        ui.add_space(8.0);
        ui.label(RichText::new("Tab  add operator").weak().small());
        ui.label(RichText::new("Double-click  set viewer").weak().small());
        ui.label(
            RichText::new("B  bypass    Space  play/pause")
                .weak()
                .small(),
        );
        ui.label(RichText::new("H  reset view    Del  delete").weak().small());
        return;
    };

    let path = app.graph.path(id);
    let op_type = app.graph.node(id).op_type.clone();
    let summary = app
        .registry
        .get(&op_type)
        .map(|d| d.summary.to_string())
        .unwrap_or_default();

    ui.horizontal(|ui| {
        ui.strong(&path);
        ui.label(RichText::new(&op_type).weak());
    });
    ui.label(RichText::new(summary).weak().small());

    ui.horizontal(|ui| {
        let mut bypass = app.graph.node(id).flags.bypass;
        if ui.checkbox(&mut bypass, "Bypass").changed() {
            app.graph.node_mut(id).flags.bypass = bypass;
        }
        let mut display = app.graph.node(id).flags.display;
        if ui
            .checkbox(&mut display, "Viewer")
            .on_hover_text("Cook this node even when nothing downstream needs it")
            .changed()
        {
            app.graph.node_mut(id).flags.display = display;
        }
        if ui.button("Set as output").clicked() {
            app.viewer = Some(id);
        }
    });

    ui.horizontal(|ui| {
        let mut name = app.graph.node(id).name.clone();
        ui.label("Name");
        if ui.text_edit_singleline(&mut name).changed() {
            let parent = app.graph.node(id).parent.unwrap_or(app.graph.root());
            let trimmed = name.trim().to_string();
            if !trimmed.is_empty() && !app.graph.name_taken(parent, &trimmed) {
                app.graph.node_mut_quiet(id).name = trimmed;
            }
        }
    });

    ui.separator();

    let keys: Vec<String> = app.graph.node(id).params.keys().cloned().collect();
    let eval_ctx = app.time.eval_ctx();

    for key in keys {
        // A shader source gets a code editor across the full panel width
        // rather than a one-line text field, with its compile error directly
        // underneath. Live-coding is only usable if the error is where you
        // are looking.
        if key == "source" {
            shader_editor(app, ui, id);
            continue;
        }

        let (label, mode, mut value, mut expression, range, menu, error, evaluated) = {
            let p = &app.graph.node(id).params[&key];
            let label = if p.label.is_empty() {
                key.clone()
            } else {
                p.label.clone()
            };
            (
                label,
                p.mode,
                p.value.clone(),
                p.expression.clone(),
                p.range,
                p.menu.clone(),
                p.error().map(|e| e.to_string()),
                p.eval(&eval_ctx),
            )
        };

        let mut changed = false;
        let mut new_mode = mode;

        ui.horizontal(|ui| {
            ui.add_sized([120.0, 18.0], egui::Label::new(&label).truncate());

            // Mode button — the whole point of the panel.
            let (glyph, tint) = match mode {
                ParamMode::Constant => ("=", Color32::from_rgb(140, 142, 150)),
                ParamMode::Expression => ("ƒ", Color32::from_rgb(150, 200, 255)),
                ParamMode::Export => ("→", Color32::from_rgb(150, 220, 150)),
                ParamMode::Bind => ("↔", Color32::from_rgb(220, 180, 120)),
            };
            let btn = ui
                .add(
                    egui::Button::new(RichText::new(glyph).color(tint))
                        .min_size([20.0, 18.0].into()),
                )
                .on_hover_text("Constant / Expression");
            if btn.clicked() {
                new_mode = match mode {
                    ParamMode::Constant => ParamMode::Expression,
                    _ => ParamMode::Constant,
                };
                changed = true;
            }

            if mode == ParamMode::Expression {
                if ui.text_edit_singleline(&mut expression).changed() {
                    changed = true;
                    new_mode = ParamMode::Expression;
                }
            } else {
                changed |= value_widget(ui, &key, &mut value, range, menu.as_deref());
            }
        });

        if mode == ParamMode::Expression {
            ui.horizontal(|ui| {
                ui.add_space(126.0);
                match &error {
                    Some(e) => {
                        ui.colored_label(
                            Color32::from_rgb(230, 120, 120),
                            RichText::new(e).small(),
                        );
                    }
                    None => {
                        ui.label(
                            RichText::new(format!("= {}", format_value(&evaluated)))
                                .weak()
                                .small(),
                        );
                    }
                }
            });
        }

        if changed {
            let node = app.graph.node_mut(id);
            let p = node.params.get_mut(&key).unwrap();
            match new_mode {
                ParamMode::Expression => {
                    p.expression = expression;
                    p.mode = ParamMode::Expression;
                    p.recompile();
                }
                _ => {
                    p.value = value;
                    p.mode = ParamMode::Constant;
                    p.recompile();
                }
            }
        }
    }
}

/// The code editor for a GLSL TOP's `source` parameter.
fn shader_editor(app: &mut OtdApp, ui: &mut egui::Ui, id: otd_core::NodeId) {
    let mut source = app.graph.node(id).params["source"].value.as_str();
    let is_glsl = app
        .graph
        .node(id)
        .params
        .get("language")
        .map(|p| p.value.as_str() == "glsl")
        .unwrap_or(false);

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.strong("Source");
        ui.label(
            RichText::new(if is_glsl {
                "Shadertoy GLSL — write mainImage(); iTime, iResolution, iFrame are provided"
            } else {
                "WGSL fragment body — in.uv, U.time.x, U.res, sample0/1(uv), U.p0..p3"
            })
            .weak()
            .small(),
        );
    });

    let response = ui.add(
        egui::TextEdit::multiline(&mut source)
            .code_editor()
            .desired_rows(16)
            .desired_width(f32::INFINITY),
    );
    if response.changed() {
        let _ = app.graph.set_param(id, "source", Value::Str(source));
    }

    // The compile error comes from the GPU engine, not the parameter, because
    // the shader is only compiled when the node cooks.
    match app.top.shader_error(id) {
        Some(err) => {
            ui.colored_label(
                Color32::from_rgb(235, 120, 120),
                RichText::new(err).small().monospace(),
            );
            ui.label(
                RichText::new("holding the last shader that compiled")
                    .weak()
                    .small(),
            );
        }
        None => {
            ui.colored_label(
                Color32::from_rgb(130, 200, 140),
                RichText::new("compiled").small(),
            );
        }
    }
    ui.add_space(4.0);
}

fn format_value(v: &Value) -> String {
    match v {
        Value::Float(f) => format!("{f:.4}"),
        other => other.as_str(),
    }
}

/// Draw the editor appropriate to the value's type. Returns true on edit.
fn value_widget(
    ui: &mut egui::Ui,
    key: &str,
    value: &mut Value,
    range: Option<(f64, f64)>,
    menu: Option<&[String]>,
) -> bool {
    if let Some(items) = menu {
        let mut current = value.as_str();
        let mut changed = false;
        egui::ComboBox::from_id_salt(key)
            .selected_text(&current)
            .show_ui(ui, |ui| {
                for item in items {
                    if ui.selectable_label(current == *item, item).clicked() {
                        current = item.clone();
                        changed = true;
                    }
                }
            });
        if changed {
            *value = Value::Str(current);
        }
        return changed;
    }

    match value {
        Value::Float(v) => match range {
            Some((lo, hi)) => ui.add(egui::Slider::new(v, lo..=hi)).changed(),
            None => ui.add(egui::DragValue::new(v).speed(0.01)).changed(),
        },
        Value::Int(v) => match range {
            Some((lo, hi)) => ui
                .add(egui::Slider::new(v, lo as i64..=hi as i64))
                .changed(),
            None => ui.add(egui::DragValue::new(v).speed(1.0)).changed(),
        },
        Value::Bool(v) => ui.checkbox(v, "").changed(),
        Value::Str(v) => ui.text_edit_singleline(v).changed(),
        Value::Vec2(v) => {
            let mut changed = false;
            for c in v.iter_mut() {
                changed |= ui.add(egui::DragValue::new(c).speed(0.01)).changed();
            }
            changed
        }
        Value::Vec3(v) => {
            let mut changed = false;
            for c in v.iter_mut() {
                changed |= ui.add(egui::DragValue::new(c).speed(0.01)).changed();
            }
            changed
        }
        Value::Vec4(v) => {
            // A four-component parameter named "color" gets a colour picker;
            // anything else gets four numbers.
            if key.contains("color") {
                let mut rgba = [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32];
                if ui.color_edit_button_rgba_unmultiplied(&mut rgba).changed() {
                    *v = [
                        rgba[0] as f64,
                        rgba[1] as f64,
                        rgba[2] as f64,
                        rgba[3] as f64,
                    ];
                    return true;
                }
                false
            } else {
                let mut changed = false;
                for c in v.iter_mut() {
                    changed |= ui.add(egui::DragValue::new(c).speed(0.01)).changed();
                }
                changed
            }
        }
    }
}
