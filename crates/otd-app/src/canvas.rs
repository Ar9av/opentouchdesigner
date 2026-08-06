//! The network editor.
//!
//! PLAN.md §6 flags this as the layer worth owning outright, because it is the
//! product surface. Two things it must get right and does:
//!
//!  * **Every node shows its real output at frame rate.** The node body is the
//!    operator's texture, not a preview render. The network is the debugger.
//!  * **Cook state is legible.** Animated nodes are marked, cached nodes are
//!    dimmed, and each node carries its own cook time.

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use otd_core::{Family, NodeId};

use crate::app::{CreateDialog, OtdApp};

pub const NODE_W: f32 = 176.0;
pub const HEADER_H: f32 = 20.0;
pub const BODY_H: f32 = 99.0;
pub const FOOTER_H: f32 = 16.0;
pub const NODE_H: f32 = HEADER_H + BODY_H + FOOTER_H;
const PORT_R: f32 = 5.0;

#[derive(Clone, Copy, Debug)]
pub struct CanvasView {
    pub pan: Vec2,
    pub zoom: f32,
}

impl Default for CanvasView {
    fn default() -> Self {
        CanvasView {
            pan: Vec2::ZERO,
            zoom: 1.0,
        }
    }
}

impl CanvasView {
    fn to_screen(self, origin: Pos2, world: Vec2) -> Pos2 {
        origin + (world + self.pan) * self.zoom
    }
    fn to_world(self, origin: Pos2, screen: Pos2) -> Vec2 {
        (screen - origin) / self.zoom - self.pan
    }
}

#[derive(Clone, Copy, Debug)]
pub enum DragState {
    Node { id: NodeId, grab: Vec2 },
    Wire { from: NodeId },
}

pub fn show(app: &mut OtdApp, ui: &mut egui::Ui) {
    egui::CentralPanel::no_frame()
        .frame(egui::Frame::NONE.fill(Color32::from_rgb(28, 29, 33)))
        .show(ui, |ui| {
            let rect = ui.available_rect_before_wrap();
            let response = ui.allocate_rect(rect, Sense::click_and_drag());
            let origin = rect.min;

            handle_view_input(app, ui, &response, origin, rect);
            draw_grid(ui, rect, app.view, origin);

            let nodes = app.graph.walk();
            let mut node_rects: Vec<(NodeId, Rect)> = Vec::new();
            for id in &nodes {
                if *id == app.graph.root() {
                    continue;
                }
                let p = app.graph.node(*id).pos;
                let min = app.view.to_screen(origin, Vec2::new(p[0], p[1]));
                let size = Vec2::new(NODE_W, NODE_H) * app.view.zoom;
                node_rects.push((*id, Rect::from_min_size(min, size)));
            }

            // Remember what is on screen: these are the cook roots next frame.
            app.visible = node_rects
                .iter()
                .filter(|(_, r)| rect.intersects(*r))
                .map(|(id, _)| *id)
                .collect();

            draw_wires(app, ui, origin, &node_rects);
            for (id, node_rect) in &node_rects {
                draw_node(app, ui, *id, *node_rect, origin);
            }
            draw_pending_wire(app, ui, origin, &node_rects);
            handle_keys(app, ui, origin, rect);
            create_dialog(app, ui, origin);
        });
}

fn handle_view_input(
    app: &mut OtdApp,
    ui: &egui::Ui,
    response: &egui::Response,
    origin: Pos2,
    rect: Rect,
) {
    // Zoom about the pointer, so the thing under the cursor stays put.
    if let Some(pointer) = ui.ctx().pointer_hover_pos() {
        if rect.contains(pointer) {
            let scroll = ui.ctx().input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.0 {
                let before = app.view.to_world(origin, pointer);
                app.view.zoom = (app.view.zoom * (1.0 + scroll * 0.002)).clamp(0.15, 3.0);
                let after = app.view.to_world(origin, pointer);
                app.view.pan += after - before;
            }
        }
    }
    // Dragging empty background pans.
    if response.dragged() && app.drag.is_none() {
        app.view.pan += response.drag_delta() / app.view.zoom;
    }
    if response.clicked() {
        app.selected = None;
    }
}

fn draw_grid(ui: &egui::Ui, rect: Rect, view: CanvasView, origin: Pos2) {
    let painter = ui.painter_at(rect);
    let spacing = 40.0 * view.zoom;
    if spacing < 6.0 {
        return;
    }
    let offset = (view.pan * view.zoom).to_pos2();
    let color = Color32::from_rgb(40, 42, 48);
    let mut x = origin.x + offset.x % spacing;
    while x < rect.max.x {
        painter.line_segment(
            [Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)],
            Stroke::new(1.0, color),
        );
        x += spacing;
    }
    let mut y = origin.y + offset.y % spacing;
    while y < rect.max.y {
        painter.line_segment(
            [Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)],
            Stroke::new(1.0, color),
        );
        y += spacing;
    }
}

fn input_port_pos(node_rect: Rect, index: usize, count: usize) -> Pos2 {
    let span = node_rect.height() - HEADER_H;
    let step = span / (count + 1) as f32;
    Pos2::new(
        node_rect.min.x,
        node_rect.min.y + HEADER_H + step * (index + 1) as f32,
    )
}

fn output_port_pos(node_rect: Rect) -> Pos2 {
    Pos2::new(node_rect.max.x, node_rect.center().y)
}

fn family_color(f: Family) -> Color32 {
    let c = f.color();
    Color32::from_rgb(c[0], c[1], c[2])
}

fn draw_wires(app: &OtdApp, ui: &egui::Ui, _origin: Pos2, rects: &[(NodeId, Rect)]) {
    let painter = ui.painter();
    let find = |id: NodeId| rects.iter().find(|(i, _)| *i == id).map(|(_, r)| *r);
    for (dst, dst_rect) in rects {
        let node = app.graph.node(*dst);
        let count = node.inputs.len();
        for (i, slot) in node.inputs.iter().enumerate() {
            let Some(src) = slot else { continue };
            let Some(src_rect) = find(*src) else { continue };
            let a = output_port_pos(src_rect);
            let b = input_port_pos(*dst_rect, i, count);
            let animated = app.cook.is_time_dependent(*src);
            let color = family_color(node.family);
            let color = if animated {
                color
            } else {
                color.gamma_multiply(0.55)
            };
            painter.add(bezier(
                a,
                b,
                Stroke::new(2.0 * app.view.zoom.max(0.5), color),
            ));
        }
    }
}

fn bezier(a: Pos2, b: Pos2, stroke: Stroke) -> egui::Shape {
    let dx = ((b.x - a.x).abs() * 0.5).max(30.0);
    egui::Shape::CubicBezier(egui::epaint::CubicBezierShape::from_points_stroke(
        [a, a + Vec2::new(dx, 0.0), b - Vec2::new(dx, 0.0), b],
        false,
        Color32::TRANSPARENT,
        stroke,
    ))
}

fn draw_pending_wire(app: &OtdApp, ui: &egui::Ui, _origin: Pos2, rects: &[(NodeId, Rect)]) {
    let Some(DragState::Wire { from }) = app.drag else {
        return;
    };
    let Some((_, src_rect)) = rects.iter().find(|(i, _)| *i == from) else {
        return;
    };
    let Some(pointer) = ui.ctx().pointer_latest_pos() else {
        return;
    };
    let color = family_color(app.graph.node(from).family);
    ui.painter().add(bezier(
        output_port_pos(*src_rect),
        pointer,
        Stroke::new(2.0, color),
    ));
}

fn draw_node(app: &mut OtdApp, ui: &mut egui::Ui, id: NodeId, rect: Rect, origin: Pos2) {
    let zoom = app.view.zoom;
    let painter = ui.painter();
    let node = app.graph.node(id);
    let family = node.family;
    let selected = app.selected == Some(id);
    let is_viewer = app.viewer == Some(id);
    let animated = app.cook.is_time_dependent(id);
    let bypassed = node.flags.bypass;
    let name = node.name.clone();
    let op_label = node.op_type.clone();
    let input_count = node.inputs.len();
    let input_slots: Vec<Option<NodeId>> = node.inputs.clone();
    let cook_us = app.cook.last_cook_us(id);
    let shader_error = app.top.shader_error(id).is_some();

    let radius = CornerRadius::same(4);
    painter.rect_filled(rect, radius, Color32::from_rgb(46, 48, 55));

    // Header, tinted by family so wire type is readable at a glance.
    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), HEADER_H * zoom));
    painter.rect_filled(header, radius, family_color(family).gamma_multiply(0.45));
    if zoom > 0.4 {
        painter.text(
            header.left_center() + Vec2::new(6.0 * zoom, 0.0),
            Align2::LEFT_CENTER,
            &name,
            FontId::proportional(11.0 * zoom),
            Color32::from_rgb(235, 236, 240),
        );
    }

    // Body: the operator's live output.
    let body = Rect::from_min_max(
        Pos2::new(rect.min.x + 2.0, header.max.y + 2.0),
        Pos2::new(rect.max.x - 2.0, rect.max.y - FOOTER_H * zoom),
    );
    painter.rect_filled(body, 2.0, Color32::from_rgb(18, 18, 20));
    if let Some((tid, size)) = app.thumbnail(id) {
        let aspect = size[0] as f32 / size[1].max(1) as f32;
        let draw = if aspect > body.width() / body.height() {
            let h = body.width() / aspect;
            Rect::from_center_size(body.center(), Vec2::new(body.width(), h))
        } else {
            let w = body.height() * aspect;
            Rect::from_center_size(body.center(), Vec2::new(w, body.height()))
        };
        ui.painter().image(
            tid,
            draw,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    let painter = ui.painter();
    if zoom > 0.4 {
        painter.text(
            Pos2::new(rect.min.x + 6.0 * zoom, rect.max.y - FOOTER_H * zoom * 0.5),
            Align2::LEFT_CENTER,
            &op_label,
            FontId::proportional(9.5 * zoom),
            Color32::from_rgb(150, 152, 160),
        );
        if app.show_perf && cook_us > 0 {
            painter.text(
                Pos2::new(rect.max.x - 6.0 * zoom, rect.max.y - FOOTER_H * zoom * 0.5),
                Align2::RIGHT_CENTER,
                format!("{:.2} ms", cook_us as f64 / 1000.0),
                FontId::proportional(9.5 * zoom),
                if animated {
                    Color32::from_rgb(150, 190, 150)
                } else {
                    Color32::from_rgb(110, 112, 120)
                },
            );
        }
    }

    // Status marks: animated (re-cooks every frame), bypassed, viewed.
    if animated {
        painter.circle_filled(
            Pos2::new(rect.max.x - 8.0 * zoom, rect.min.y + HEADER_H * zoom * 0.5),
            3.0 * zoom,
            Color32::from_rgb(120, 220, 140),
        );
    }
    if bypassed {
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(2.0, Color32::from_rgb(220, 190, 90)),
            StrokeKind::Outside,
        );
    }
    if shader_error {
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(2.0, Color32::from_rgb(220, 90, 90)),
            StrokeKind::Outside,
        );
        if zoom > 0.4 {
            painter.text(
                rect.center_bottom() - Vec2::new(0.0, FOOTER_H * zoom + 4.0),
                Align2::CENTER_BOTTOM,
                "shader error",
                FontId::proportional(10.0 * zoom),
                Color32::from_rgb(240, 140, 140),
            );
        }
    }
    if is_viewer {
        painter.rect_stroke(
            rect.expand(2.0),
            radius,
            Stroke::new(1.5, Color32::from_rgb(120, 170, 240)),
            StrokeKind::Outside,
        );
    }
    if selected {
        painter.rect_stroke(
            rect.expand(1.0),
            radius,
            Stroke::new(2.0, Color32::from_rgb(240, 240, 250)),
            StrokeKind::Outside,
        );
    }

    // ---- ports
    let out_pos = output_port_pos(rect);
    painter.circle_filled(out_pos, PORT_R * zoom, family_color(family));
    for (i, slot) in input_slots.iter().enumerate() {
        let p = input_port_pos(rect, i, input_count);
        let filled = slot.is_some();
        painter.circle_filled(
            p,
            PORT_R * zoom,
            if filled {
                family_color(family)
            } else {
                Color32::from_rgb(80, 82, 90)
            },
        );
    }

    // ---- interaction
    let out_rect = Rect::from_center_size(out_pos, Vec2::splat(PORT_R * 3.0 * zoom));
    let out_resp = ui.interact(
        out_rect,
        egui::Id::new(("out", id)),
        Sense::click_and_drag(),
    );
    if out_resp.drag_started() {
        app.drag = Some(DragState::Wire { from: id });
    }

    for i in 0..input_count {
        let p = input_port_pos(rect, i, input_count);
        let r = Rect::from_center_size(p, Vec2::splat(PORT_R * 3.0 * zoom));
        let resp = ui.interact(r, egui::Id::new(("in", id, i)), Sense::click_and_drag());
        if resp.hovered() {
            if let Some(DragState::Wire { from }) = app.drag {
                if ui.ctx().input(|inp| inp.pointer.any_released()) {
                    match app.graph.connect(from, id, i) {
                        Ok(()) => app.status = String::new(),
                        Err(e) => app.status = e.to_string(),
                    }
                    app.drag = None;
                }
            }
        }
        if resp.clicked() {
            let _ = app.graph.disconnect(id, i);
        }
        if !resp.hovered() && input_count > 1 {
            // Label multi-input operators on hover of the node itself.
        }
    }

    let body_resp = ui.interact(rect, egui::Id::new(("node", id)), Sense::click_and_drag());
    if body_resp.clicked() {
        app.selected = Some(id);
    }
    if body_resp.double_clicked() {
        app.viewer = Some(id);
    }
    if body_resp.drag_started() {
        if let Some(pointer) = ui.ctx().pointer_interact_pos() {
            let world = app.view.to_world(origin, pointer);
            let p = app.graph.node(id).pos;
            app.drag = Some(DragState::Node {
                id,
                grab: world - Vec2::new(p[0], p[1]),
            });
            app.selected = Some(id);
        }
    }
    if body_resp.hovered() {
        let summary = app
            .registry
            .get(&op_label)
            .map(|d| d.summary)
            .unwrap_or_default();
        body_resp.on_hover_text(format!("{op_label}\n{summary}"));
    }

    // Dragging a node moves it without dirtying it — layout is not a cook
    // input.
    if let Some(DragState::Node { id: drag_id, grab }) = app.drag {
        if drag_id == id {
            if let Some(pointer) = ui.ctx().pointer_latest_pos() {
                let world = app.view.to_world(origin, pointer) - grab;
                app.graph.node_mut_quiet(id).pos = [world.x, world.y];
            }
        }
    }
}

fn handle_keys(app: &mut OtdApp, ui: &egui::Ui, origin: Pos2, rect: Rect) {
    // Releasing the mouse ends any drag. A wire dropped on empty canvas is
    // simply cancelled; the connection itself is made by the input port's own
    // hover handler, which runs before this.
    if ui.ctx().input(|i| i.pointer.any_released()) {
        app.drag = None;
    }
    if ui.ctx().egui_wants_keyboard_input() {
        return;
    }

    let pointer = ui.ctx().pointer_hover_pos().unwrap_or(rect.center());
    ui.ctx().input(|i| {
        if i.key_pressed(egui::Key::Tab) {
            app.create_dialog = Some(CreateDialog {
                filter: String::new(),
                world_pos: app.view.to_world(origin, pointer),
                focus: true,
                connect_from: app.selected,
            });
        }
        if i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace) {
            app.delete_selected();
        }
        if i.key_pressed(egui::Key::B) {
            if let Some(id) = app.selected {
                let now = app.graph.node(id).flags.bypass;
                app.graph.node_mut(id).flags.bypass = !now;
            }
        }
        if i.key_pressed(egui::Key::Space) {
            app.playing = !app.playing;
        }
        if i.key_pressed(egui::Key::H) {
            app.view = CanvasView::default();
        }
    });
}

fn create_dialog(app: &mut OtdApp, ui: &egui::Ui, _origin: Pos2) {
    let Some(dialog) = &mut app.create_dialog else {
        return;
    };
    let mut open = true;
    let mut close_requested = false;
    let mut chosen: Option<String> = None;
    let filter = dialog.filter.to_lowercase();
    let world_pos = dialog.world_pos;
    let connect_from = dialog.connect_from;
    let focus = std::mem::take(&mut dialog.focus);

    egui::Window::new("Add operator")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 80.0))
        .show(ui.ctx(), |ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut app.create_dialog.as_mut().unwrap().filter)
                    .hint_text("type to filter…")
                    .desired_width(320.0),
            );
            if focus {
                edit.request_focus();
            }
            ui.separator();
            let matches: Vec<(&str, &str, &str)> = app
                .registry
                .iter()
                .filter(|d| {
                    filter.is_empty()
                        || d.type_name.to_lowercase().contains(&filter)
                        || d.label.to_lowercase().contains(&filter)
                })
                .map(|d| (d.type_name, d.label, d.summary))
                .collect();

            egui::ScrollArea::vertical()
                .max_height(280.0)
                .show(ui, |ui| {
                    for (type_name, label, summary) in &matches {
                        if ui
                            .selectable_label(false, format!("{label}  ({type_name})"))
                            .on_hover_text(*summary)
                            .clicked()
                        {
                            chosen = Some(type_name.to_string());
                        }
                    }
                });

            if ui.ctx().input(|i| i.key_pressed(egui::Key::Enter)) {
                if let Some((type_name, ..)) = matches.first() {
                    chosen = Some(type_name.to_string());
                }
            }
            if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
                close_requested = true;
            }
        });
    let open = open && !close_requested;

    if let Some(type_name) = chosen {
        if let Some(new_id) = app.create_node(&type_name, world_pos) {
            // Auto-wire from the previously selected node, the way a
            // chain gets built in practice.
            if let Some(src) = connect_from.filter(|s| app.graph.contains(*s)) {
                let _ = app.graph.connect(src, new_id, 0);
            }
        }
        app.create_dialog = None;
    } else if !open {
        app.create_dialog = None;
    }
}
