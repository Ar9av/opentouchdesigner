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
    pub fn to_world(self, origin: Pos2, screen: Pos2) -> Vec2 {
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
            // Dropped files are placed where the pointer is, which means the
            // drop handler needs to know where the network was drawn.
            app.canvas_rect = rect;

            handle_view_input(app, ui, &response, origin, rect);
            app.input_state = sample_input(ui, rect);
            draw_grid(ui, rect, app.view, origin);

            let nodes: Vec<NodeId> = app.graph.children(app.current).to_vec();
            let mut node_rects: Vec<(NodeId, Rect)> = Vec::new();
            for id in &nodes {
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
            if nodes.is_empty() {
                empty_hint(app, ui, rect);
            }
            handle_keys(app, ui, origin, rect);
            create_dialog(app, ui, origin);
        });
}

/// Which node is under a screen position, if any. Used by the drop handler,
/// which runs outside the canvas and so has no node rectangles of its own.
pub fn node_at(app: &OtdApp, pos: Pos2) -> Option<NodeId> {
    if !app.canvas_rect.contains(pos) {
        return None;
    }
    let origin = app.canvas_rect.min;
    let size = Vec2::new(NODE_W, NODE_H) * app.view.zoom;
    // Reverse order, so the node drawn last — the one on top — wins.
    app.graph
        .children(app.current)
        .iter()
        .rev()
        .copied()
        .find(|id| {
            let p = app.graph.node(*id).pos;
            let min = app.view.to_screen(origin, Vec2::new(p[0], p[1]));
            Rect::from_min_size(min, size).contains(pos)
        })
}

/// An empty network is the one screen where the program tells you nothing at
/// all about what to do next. This is that screen doing its job.
fn empty_hint(app: &mut OtdApp, ui: &mut egui::Ui, rect: Rect) {
    let painter = ui.painter_at(rect);
    let centre = rect.center();
    painter.text(
        centre - Vec2::new(0.0, 74.0),
        Align2::CENTER_CENTER,
        "Drop a video, image or audio file here",
        FontId::proportional(20.0),
        Color32::from_rgb(190, 196, 210),
    );
    painter.text(
        centre - Vec2::new(0.0, 46.0),
        Align2::CENTER_CENTER,
        "it starts playing, and everything downstream is yours to build",
        FontId::proportional(13.0),
        Color32::from_gray(125),
    );

    let button = Rect::from_center_size(centre, Vec2::new(190.0, 30.0));
    if ui.put(button, egui::Button::new("Import media…")).clicked() {
        crate::media::import_dialog(app);
    }
    let webcam = Rect::from_center_size(centre + Vec2::new(0.0, 38.0), Vec2::new(190.0, 30.0));
    if ui.put(webcam, egui::Button::new("Use webcam")).clicked() {
        crate::media::add_webcam(app);
    }

    painter.text(
        centre + Vec2::new(0.0, 84.0),
        Align2::CENTER_CENTER,
        "or press Tab for any of the 126 operators",
        FontId::proportional(13.0),
        Color32::from_gray(110),
    );
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
        app.clear_selection();
    }
}

/// Snapshot the pointer and keyboard for the Mouse In and Keyboard In CHOPs.
///
/// The editor is the only thing that talks to the window system; `otd-chop`
/// just reads whatever is handed to it.
fn sample_input(ui: &egui::Ui, rect: Rect) -> otd_chop::InputState {
    let ctx = ui.ctx();
    let pos = ctx.pointer_latest_pos().unwrap_or(rect.center());
    ctx.input(|i| otd_chop::InputState {
        // Centre origin, Y up — the convention a Transform TOP expects.
        mouse: [
            (pos.x - rect.min.x) / rect.width().max(1.0) - 0.5,
            0.5 - (pos.y - rect.min.y) / rect.height().max(1.0),
        ],
        buttons: [
            i.pointer.primary_down(),
            i.pointer.secondary_down(),
            i.pointer.middle_down(),
        ],
        wheel: i.smooth_scroll_delta.y,
        keys: i
            .keys_down
            .iter()
            .map(|k| format!("{k:?}").to_lowercase())
            .collect(),
    })
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
            // Tinted by what the wire carries, not by what it arrives at, so a
            // DAT feeding a DAT to CHOP still reads as a DAT wire.
            let color = family_color(node.input_families.get(i).copied().unwrap_or(node.family));
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

/// Colours for channels on a node body and in the channel list, so the same
/// channel is the same colour in both places.
pub fn channel_color(index: usize) -> Color32 {
    const PALETTE: [[u8; 3]; 6] = [
        [126, 190, 126],
        [126, 170, 220],
        [220, 180, 110],
        [200, 130, 200],
        [220, 130, 130],
        [140, 210, 200],
    ];
    let c = PALETTE[index % PALETTE.len()];
    Color32::from_rgb(c[0], c[1], c[2])
}

fn draw_chop_body(app: &OtdApp, ui: &egui::Ui, id: NodeId, body: Rect, zoom: f32) {
    let Some(data) = app.engines.chop_data(id) else {
        return;
    };
    if data.channels.is_empty() {
        return;
    }
    let painter = ui.painter_at(body);

    // One shared vertical scale across channels, so their relative sizes are
    // readable. A flat signal still gets a visible line rather than filling
    // the box.
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for ch in &data.channels {
        lo = lo.min(ch.min());
        hi = hi.max(ch.max());
    }
    if !lo.is_finite() || !hi.is_finite() {
        return;
    }
    let span = (hi - lo).max(1e-3);
    let (lo, hi) = (lo - span * 0.1, hi + span * 0.1);

    let y_of = |v: f32| {
        let t = (v - lo) / (hi - lo).max(1e-6);
        body.max.y - t * body.height()
    };

    // Zero line, when zero is in range.
    if lo < 0.0 && hi > 0.0 {
        painter.line_segment(
            [
                Pos2::new(body.min.x, y_of(0.0)),
                Pos2::new(body.max.x, y_of(0.0)),
            ],
            Stroke::new(1.0, Color32::from_rgb(50, 52, 58)),
        );
    }

    for (ci, ch) in data.channels.iter().enumerate().take(8) {
        let color = channel_color(ci);
        let n = ch.samples.len();
        if n == 0 {
            continue;
        }
        if n == 1 {
            // A time-sliced control channel is one sample per frame: draw it
            // as a level rather than as a dot in the corner.
            let y = y_of(ch.samples[0]);
            painter.line_segment(
                [Pos2::new(body.min.x, y), Pos2::new(body.max.x, y)],
                Stroke::new(1.5 * zoom.max(0.5), color),
            );
            continue;
        }
        let step = body.width() / (n - 1) as f32;
        let points: Vec<Pos2> = ch
            .samples
            .iter()
            .enumerate()
            .map(|(i, v)| Pos2::new(body.min.x + i as f32 * step, y_of(*v)))
            .collect();
        painter.add(egui::Shape::line(
            points,
            Stroke::new(1.2 * zoom.max(0.5), color),
        ));
    }

    if zoom > 0.6 {
        painter.text(
            body.left_top() + Vec2::new(3.0, 2.0),
            Align2::LEFT_TOP,
            format!("{} ch  {} samp", data.num_channels(), data.num_samples()),
            FontId::proportional(8.5 * zoom),
            Color32::from_rgb(120, 122, 130),
        );
    }
}

/// A DAT's viewer is the first few rows of its table, which is usually
/// enough to see whether the data is what you expected.
fn draw_dat_body(app: &OtdApp, ui: &egui::Ui, id: NodeId, body: Rect, zoom: f32) {
    let Some(data) = app.engines.dat_data(id) else {
        return;
    };
    if zoom < 0.45 || data.rows.is_empty() {
        return;
    }
    let painter = ui.painter_at(body);
    let line = 11.0 * zoom;
    let font = FontId::monospace(9.0 * zoom);
    let max_rows = ((body.height() / line) as usize).max(1);

    for (r, row) in data.rows.iter().take(max_rows).enumerate() {
        // The first row of a table is nearly always headings.
        let colour = if r == 0 && !data.is_text {
            Color32::from_rgb(200, 202, 212)
        } else {
            Color32::from_rgb(150, 152, 162)
        };
        let text: String = row.join("  ").chars().take(30).collect();
        painter.text(
            body.left_top() + Vec2::new(4.0, 2.0 + r as f32 * line),
            Align2::LEFT_TOP,
            text,
            font.clone(),
            colour,
        );
    }
    if data.num_rows() > max_rows {
        painter.text(
            body.right_bottom() - Vec2::new(4.0, 2.0),
            Align2::RIGHT_BOTTOM,
            format!("+{} rows", data.num_rows() - max_rows),
            FontId::proportional(8.5 * zoom),
            Color32::from_rgb(110, 112, 122),
        );
    }
}

fn draw_node(app: &mut OtdApp, ui: &mut egui::Ui, id: NodeId, rect: Rect, origin: Pos2) {
    let zoom = app.view.zoom;
    let painter = ui.painter();
    let node = app.graph.node(id);
    let family = node.family;
    let selected = app.is_selected(id);
    let primary = app.selected == Some(id);
    let is_viewer = app.viewer == Some(id);
    let animated = app.cook.is_time_dependent(id);
    let bypassed = node.flags.bypass;
    let name = node.name.clone();
    let op_label = node.op_type.clone();
    let input_count = node.inputs.len();
    let input_slots: Vec<Option<NodeId>> = node.inputs.clone();
    // A converter's input accepts somebody else's family, and the port has to
    // say so — the family colours are the type system made visible, and a
    // green port that only takes a DAT would be worse than no colour at all.
    let input_families: Vec<Family> = node.input_families.clone();
    let cook_us = app.cook.last_cook_us(id);
    let status = app.engines.node_status(&app.graph, id);
    let shader_error = status.is_some();

    let radius = CornerRadius::same(4);
    painter.rect_filled(rect, radius, Color32::from_rgb(46, 48, 55));

    // Header, tinted by family so wire type is readable at a glance.
    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), HEADER_H * zoom));
    painter.rect_filled(header, radius, family_color(family).gamma_multiply(0.45));
    if zoom > 0.4 {
        // Clipped to the header: a long name — and a name taken from a
        // filename is often long — otherwise runs out over the wires and the
        // node beside it, and reads as a rendering bug.
        painter
            .with_clip_rect(header.shrink2(Vec2::new(4.0 * zoom, 0.0)))
            .text(
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
    match family {
        Family::Top => {
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
        }
        // A CHOP's viewer is its waveform, for the same reason a TOP's is its
        // texture: you should be able to see what an operator is doing
        // without opening anything.
        Family::Chop => draw_chop_body(app, ui, id, body, zoom),
        Family::Dat => draw_dat_body(app, ui, id, body, zoom),
        _ => {}
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
            // The first few words of the real message, so the node itself
            // says what is wrong rather than just that something is.
            let text: String = status
                .as_deref()
                .unwrap_or("error")
                .chars()
                .take(34)
                .collect();
            painter.text(
                rect.center_bottom() - Vec2::new(0.0, FOOTER_H * zoom + 4.0),
                Align2::CENTER_BOTTOM,
                text,
                FontId::proportional(9.5 * zoom),
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
        // The primary is drawn brighter than the rest of the selection: it is
        // the one the parameter panel is showing and the one "it" means in an
        // assistant request, and with six nodes selected that is worth being
        // able to see.
        let (width, tone) = if primary {
            (2.0, Color32::from_rgb(240, 240, 250))
        } else {
            (1.5, Color32::from_rgb(150, 152, 170))
        };
        painter.rect_stroke(
            rect.expand(1.0),
            radius,
            Stroke::new(width, tone),
            StrokeKind::Outside,
        );
    }

    // ---- ports
    let out_pos = output_port_pos(rect);
    painter.circle_filled(out_pos, PORT_R * zoom, family_color(family));
    for (i, slot) in input_slots.iter().enumerate() {
        let p = input_port_pos(rect, i, input_count);
        let filled = slot.is_some();
        let accepts = input_families.get(i).copied().unwrap_or(family);
        painter.circle_filled(
            p,
            PORT_R * zoom,
            if filled {
                family_color(accepts)
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
                    app.edit("wire");
                    app.history.end_gesture();
                    match app.graph.connect(from, id, i) {
                        Ok(()) => app.status = String::new(),
                        Err(e) => app.status = e.to_string(),
                    }
                    app.drag = None;
                }
            }
        }
        if resp.clicked() {
            app.edit("unwire");
            app.history.end_gesture();
            let _ = app.graph.disconnect(id, i);
        }
        if !resp.hovered() && input_count > 1 {
            // Label multi-input operators on hover of the node itself.
        }
    }

    let body_resp = ui.interact(rect, egui::Id::new(("node", id)), Sense::click_and_drag());
    if body_resp.clicked() {
        // Shift or Cmd extends; a plain click replaces. Both are what every
        // other canvas does, and guessing wrong costs the whole selection.
        if ui.input(|i| i.modifiers.shift || i.modifiers.command) {
            app.select_toggle(id);
        } else {
            app.select_only(id);
        }
    }
    if body_resp.double_clicked() {
        if family == Family::Comp {
            app.enter(id);
        } else {
            app.viewer = Some(id);
        }
    }
    if body_resp.drag_started() {
        if let Some(pointer) = ui.ctx().pointer_interact_pos() {
            let world = app.view.to_world(origin, pointer);
            let p = app.graph.node(id).pos;
            app.drag = Some(DragState::Node {
                id,
                grab: world - Vec2::new(p[0], p[1]),
            });
            // Dragging a node that is already part of a selection moves the
            // whole selection; dragging an unselected one grabs just it.
            if !app.is_selected(id) {
                app.select_only(id);
            }
        }
    }
    node_menu(app, &body_resp, id);

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
                // One entry for the whole drag: the tag names the node, so
                // every frame of it coalesces.
                app.edit(&format!("move:{id:?}"));
                let world = app.view.to_world(origin, pointer) - grab;
                // The rest of the selection travels with it, keeping the
                // shape of the layout you already arranged.
                let delta = world - Vec2::new(
                    app.graph.node(id).pos[0],
                    app.graph.node(id).pos[1],
                );
                let others: Vec<NodeId> =
                    app.selection.iter().copied().filter(|o| *o != id).collect();
                for other in others {
                    let p = app.graph.node(other).pos;
                    app.graph.node_mut_quiet(other).pos =
                        [p[0] + delta.x, p[1] + delta.y];
                }
                app.graph.node_mut_quiet(id).pos = [world.x, world.y];
            }
        }
    }
}

/// Right-click on a node.
///
/// Nothing here is new capability — all of it is a key, a double-click or a
/// row in the parameter panel. It exists because none of those announce
/// themselves, and a right-click is where people look first.
fn node_menu(app: &mut OtdApp, resp: &egui::Response, id: NodeId) {
    resp.context_menu(|ui| {
        // Acting on a node you did not select would be a surprise; select it.
        // An already-selected node keeps the rest of the selection, so
        // right-click Delete on one of six selected nodes removes all six.
        if !app.is_selected(id) {
            app.select_only(id);
        }

        if ui.button("View").clicked() {
            app.viewer = Some(id);
            ui.close();
        }
        if app.graph.node(id).family == Family::Comp && ui.button("Enter").clicked() {
            app.enter(id);
            ui.close();
        }
        if app.graph.node(id).param("file").is_some() && ui.button("Replace File…").clicked() {
            crate::media::replace_file(app, id);
            ui.close();
        }

        ui.separator();
        ui.menu_button("Add Effect", |ui| crate::media::effects_menu(app, ui));

        ui.separator();
        let bypassed = app.graph.node(id).flags.bypass;
        if ui
            .selectable_label(bypassed, "Bypass")
            .on_hover_text("Pass the input straight through — B")
            .clicked()
        {
            app.edit("bypass");
            app.history.end_gesture();
            app.graph.node_mut(id).flags.bypass = !bypassed;
            ui.close();
        }
        if ui.button("Delete").clicked() {
            app.delete_selected();
            ui.close();
        }
    });
}

fn handle_keys(app: &mut OtdApp, ui: &egui::Ui, origin: Pos2, rect: Rect) {
    // Releasing the mouse ends any drag. A wire dropped on empty canvas is
    // simply cancelled; the connection itself is made by the input port's own
    // hover handler, which runs before this.
    if ui.ctx().input(|i| i.pointer.any_released()) {
        app.drag = None;
        // The gesture is over, so the next drag starts its own undo entry
        // rather than coalescing into this one.
        app.history.end_gesture();
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
                app.edit("bypass");
                app.history.end_gesture();
                app.graph.node_mut(id).flags.bypass = !now;
            }
        }
        if i.key_pressed(egui::Key::Space) {
            app.playing = !app.playing;
        }
        if i.key_pressed(egui::Key::H) {
            app.view = CanvasView::default();
        }
        if i.key_pressed(egui::Key::U) {
            app.leave();
        }
        if i.key_pressed(egui::Key::I) {
            if let Some(sel) = app.selected {
                app.enter(sel);
            }
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
                // No checkpoint: `create_node` already took one, and the
                // auto-wire is part of the same action — undoing the creation
                // has to take its wire with it.
                let _ = app.graph.connect(src, new_id, 0);
            }
        }
        app.create_dialog = None;
    } else if !open {
        app.create_dialog = None;
    }
}
