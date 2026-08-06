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

    channel_list(app, ui, id);
    component_links(app, ui, id);
    custom_param_editor(app, ui, id);
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
        // A DAT's contents deserve the same room as a shader's source.
        if key == "text" && app.graph.node(id).family == otd_core::Family::Dat {
            text_editor(app, ui, id);
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

        // A driven parameter's *displayed* value has to come from the network,
        // not from `Param::eval` with an empty context — otherwise the panel
        // shows the constant underneath while the render shows the channel.
        let evaluated = match mode {
            ParamMode::Export => app.graph.node(id).params[&key]
                .source_parts()
                .and_then(|(op, ch)| app.engines.channel_value(&app.graph, op, ch))
                .map(|v| value.coerce_from_f64(v as f64))
                .unwrap_or(evaluated),
            ParamMode::Bind => app.graph.node(id).params[&key]
                .source_parts()
                .and_then(|(op, p)| app.engines.param_value(&app.graph, op, p))
                .filter(|v| v.same_type_as(&value))
                .unwrap_or(evaluated),
            _ => evaluated,
        };

        let mut changed = false;
        let mut new_mode = mode;
        let mut clear_source = false;

        // The whole row is a drop zone: dragging a channel here exports it.
        let (_, dropped) = ui.dnd_drop_zone::<ChannelDrag, _>(egui::Frame::NONE, |ui| {
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
                    .on_hover_text(match mode {
                        ParamMode::Export => "Exported from a CHOP channel — click to release",
                        ParamMode::Bind => "Bound to another parameter — click to release",
                        _ => "Constant / Expression",
                    });
                if btn.clicked() {
                    new_mode = match mode {
                        ParamMode::Constant => ParamMode::Expression,
                        ParamMode::Expression => ParamMode::Constant,
                        // Releasing a driven parameter keeps whatever value it
                        // was showing, so the picture does not jump.
                        _ => {
                            clear_source = true;
                            value = evaluated.clone();
                            ParamMode::Constant
                        }
                    };
                    changed = true;
                }

                match mode {
                    ParamMode::Expression => {
                        if ui.text_edit_singleline(&mut expression).changed() {
                            changed = true;
                            new_mode = ParamMode::Expression;
                        }
                    }
                    ParamMode::Export | ParamMode::Bind => {
                        ui.label(
                            RichText::new(format_value(&evaluated))
                                .monospace()
                                .color(Color32::from_rgb(150, 220, 150)),
                        );
                    }
                    ParamMode::Constant => {
                        changed |= value_widget(ui, &key, &mut value, range, menu.as_deref());
                    }
                }
            });
        });

        if let Some(drag) = dropped {
            let p = app.graph.node_mut(id).params.get_mut(&key).unwrap();
            p.set_export(&drag.op_path, &drag.channel);
            app.status = format!("{label} ← {}:{}", drag.op_path, drag.channel);
            continue;
        }

        match mode {
            ParamMode::Expression => {
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
            ParamMode::Export | ParamMode::Bind => {
                ui.horizontal(|ui| {
                    ui.add_space(126.0);
                    let source = app.graph.node(id).params[&key].source.clone();
                    ui.label(RichText::new(source).weak().small().monospace());
                });
            }
            ParamMode::Constant => {}
        }

        if changed {
            let node = app.graph.node_mut(id);
            let p = node.params.get_mut(&key).unwrap();
            if clear_source {
                p.source.clear();
            }
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

/// Where a component's contents come from: a shared file, or another
/// component it tracks.
fn component_links(app: &mut OtdApp, ui: &mut egui::Ui, id: otd_core::NodeId) {
    if app.graph.node(id).family != otd_core::Family::Comp {
        return;
    }
    let external = app.graph.node(id).external.clone();
    let mut clone_of = app.graph.node(id).clone_of.clone().unwrap_or_default();

    ui.separator();
    if let Some(file) = &external {
        ui.horizontal(|ui| {
            ui.label(RichText::new("From file").weak().small());
            ui.label(RichText::new(file).monospace().small());
        });
        ui.label(
            RichText::new("edits here are local until you save the component again")
                .weak()
                .small(),
        );
    }
    ui.horizontal(|ui| {
        ui.label("Clone of");
        if ui
            .add(
                egui::TextEdit::singleline(&mut clone_of)
                    .hint_text("/master_component")
                    .desired_width(180.0),
            )
            .changed()
        {
            let trimmed = clone_of.trim().to_string();
            app.graph.set_clone(
                id,
                if trimmed.is_empty() {
                    None
                } else {
                    Some(&trimmed)
                },
            );
        }
    });
}

/// Add and remove a component's own parameters.
///
/// PLAN.md §2.3: "Custom parameters on components ARE the component API."
/// Operators inside read them as `parent.<name>`, so this panel is where a
/// network becomes a reusable thing with knobs.
fn custom_param_editor(app: &mut OtdApp, ui: &mut egui::Ui, id: otd_core::NodeId) {
    if app.graph.node(id).family != otd_core::Family::Comp {
        return;
    }
    ui.separator();
    let custom: Vec<String> = app
        .graph
        .node(id)
        .custom_params()
        .map(|(k, _)| k.clone())
        .collect();

    egui::CollapsingHeader::new(format!("Component parameters ({})", custom.len()))
        .default_open(!custom.is_empty())
        .show(ui, |ui| {
            ui.label(
                RichText::new("read inside this component as parent.<name>")
                    .weak()
                    .small(),
            );
            let mut remove = None;
            for key in &custom {
                ui.horizontal(|ui| {
                    if ui.small_button("✕").on_hover_text("Remove").clicked() {
                        remove = Some(key.clone());
                    }
                    ui.label(RichText::new(format!("parent.{key}")).monospace().small());
                });
            }
            if let Some(key) = remove {
                app.graph.remove_custom_param(id, &key);
            }

            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut app.new_param_name)
                        .hint_text("name")
                        .desired_width(110.0),
                );
                egui::ComboBox::from_id_salt("newparamtype")
                    .selected_text(&app.new_param_type)
                    .width(80.0)
                    .show_ui(ui, |ui| {
                        for t in ["float", "int", "bool", "str", "rgba"] {
                            if ui.selectable_label(app.new_param_type == t, t).clicked() {
                                app.new_param_type = t.to_string();
                            }
                        }
                    });
                if ui.button("Add").clicked() {
                    let name: String = app
                        .new_param_name
                        .trim()
                        .chars()
                        .filter(|c| c.is_alphanumeric() || *c == '_')
                        .collect::<String>()
                        .to_lowercase();
                    if !name.is_empty() {
                        let param = match app.new_param_type.as_str() {
                            "int" => otd_core::Param::int(0),
                            "bool" => otd_core::Param::bool(false),
                            "str" => otd_core::Param::str(""),
                            "rgba" => otd_core::Param::rgba([1.0, 1.0, 1.0, 1.0]),
                            _ => otd_core::Param::float(0.0).with_range(0.0, 1.0),
                        };
                        app.graph.add_custom_param(id, &name, param);
                        app.status = format!("added parent.{name}");
                        app.new_param_name.clear();
                    }
                }
            });
        });
}

/// The multiline editor for a Table or Text DAT's contents.
fn text_editor(app: &mut OtdApp, ui: &mut egui::Ui, id: otd_core::NodeId) {
    let mut text = app.graph.node(id).params["text"].value.as_str();
    let rows = app
        .engines
        .dat_data(id)
        .map(|d| format!("{} × {}", d.num_rows(), d.num_cols()))
        .unwrap_or_default();

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.strong("Contents");
        ui.label(RichText::new(rows).weak().small());
    });
    let response = ui.add(
        egui::TextEdit::multiline(&mut text)
            .code_editor()
            .desired_rows(10)
            .desired_width(f32::INFINITY),
    );
    if response.changed() {
        let _ = app.graph.set_param(id, "text", Value::Str(text));
    }
    ui.add_space(4.0);
}

/// What a dragged channel carries. Dropping it on a parameter exports it.
#[derive(Clone, Debug)]
pub struct ChannelDrag {
    pub op_path: String,
    pub channel: String,
}

/// The selected CHOP's channels, with their current values. Each row is a
/// drag source: drop one on a parameter to export it, which is the gesture
/// PLAN.md §5 calls for in Phase 2.
fn channel_list(app: &mut OtdApp, ui: &mut egui::Ui, id: otd_core::NodeId) {
    if app.graph.node(id).family != otd_core::Family::Chop {
        return;
    }
    let path = app.graph.path(id);
    let Some(data) = app.engines.chop_data(id) else {
        return;
    };
    let rows: Vec<(usize, String, f32, usize)> = data
        .channels
        .iter()
        .enumerate()
        .map(|(i, ch)| (i, ch.name.clone(), ch.last(), ch.samples.len()))
        .collect();
    let rate = data.sample_rate;

    ui.separator();
    ui.horizontal(|ui| {
        ui.strong("Channels");
        ui.label(
            RichText::new(format!("{} @ {rate:.0} Hz", rows.len()))
                .weak()
                .small(),
        );
    });
    if rows.is_empty() {
        ui.label(RichText::new("none").weak().small());
        return;
    }
    ui.label(
        RichText::new("drag a channel onto a parameter to export it")
            .weak()
            .small(),
    );

    for (i, name, value, samples) in rows {
        let payload = ChannelDrag {
            op_path: path.clone(),
            channel: name.clone(),
        };
        ui.dnd_drag_source(egui::Id::new(("chan", id, i)), payload, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, 2.0, crate::canvas::channel_color(i));
                ui.add_sized(
                    [110.0, 16.0],
                    egui::Label::new(RichText::new(&name).monospace()).truncate(),
                );
                ui.label(RichText::new(format!("{value:+.4}")).monospace());
                if samples > 1 {
                    ui.label(RichText::new(format!("×{samples}")).weak().small());
                }
            });
        });
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
    match app.engines.top.shader_error(id) {
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
