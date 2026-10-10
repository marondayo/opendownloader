//! Custom widgets: quality slider, toggle switch, setting rows, text helpers.

use crate::theme::*;
use eframe::egui::{
    self, pos2, text::LayoutJob, vec2, Align, Color32, CornerRadius, FontId, Galley, Layout, Painter, Pos2, Rect,
    Response, RichText, Sense, Stroke, Ui,
};
use odl_core::{Quality, QUALITY_STEPS};
use std::sync::Arc;

/// Lays out a single line of text, truncated with an ellipsis to `max_width`.
pub fn line_galley(painter: &Painter, text: &str, font: FontId, color: Color32, max_width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap.max_width = max_width.max(10.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    painter.layout_job(job)
}

pub fn paint_line(painter: &Painter, pos: Pos2, text: &str, font: FontId, color: Color32, max_width: f32) -> Rect {
    let galley = line_galley(painter, text, font, color, max_width);
    let rect = Rect::from_min_size(pos, galley.size());
    painter.galley(pos, galley, color);
    rect
}

/// iOS-style toggle switch.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let size = vec2(40.0, 22.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool(response.id, *on);
        let bg = lerp_color(BORDER, ACCENT, t);
        let radius = rect.height() / 2.0;
        ui.painter().rect_filled(rect, CornerRadius::same(radius as u8), bg);
        let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
        ui.painter().circle_filled(pos2(x, rect.center().y), radius - 4.0, Color32::WHITE);
    }
    response
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// A row with a title (and optional description) on the left and a control on the right.
pub fn setting_row<R>(ui: &mut Ui, title: &str, desc: Option<&str>, control: impl FnOnce(&mut Ui) -> R) -> R {
    let r = ui
        .horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let r = control(ui);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.vertical(|ui| {
                        ui.add(egui::Label::new(RichText::new(title).color(TEXT)).wrap());
                        if let Some(d) = desc {
                            ui.add(egui::Label::new(RichText::new(d).small().color(TEXT_DIM)).wrap());
                        }
                    });
                });
                r
            })
            .inner
        })
        .inner;
    ui.add_space(4.0);
    r
}

pub fn section_title(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).strong().size(15.0).color(TEXT));
    ui.add_space(2.0);
}

pub fn field_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).small().color(TEXT_DIM));
}

pub fn hint(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).small().color(TEXT_DIM)).wrap());
}

/// The 144p … 8K … Best slider from the download dialog.
///
/// Steps above the best `available` height are drawn faded (they fall back to the best one).
pub fn quality_slider(ui: &mut Ui, value: &mut Quality, available: Option<&[u32]>) -> Response {
    let n = QUALITY_STEPS.len();
    let (rect, mut resp) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::click_and_drag());
    let pad = 22.0;
    let (x0, x1) = (rect.left() + pad, rect.right() - pad);
    let line_y = rect.top() + 12.0;
    let pos_x = |i: usize| x0 + (x1 - x0) * i as f32 / (n - 1) as f32;

    if let Some(p) = resp.interact_pointer_pos() {
        if resp.clicked() || resp.dragged() {
            let t = ((p.x - x0) / (x1 - x0)).clamp(0.0, 1.0);
            let i = (t * (n - 1) as f32).round() as usize;
            if QUALITY_STEPS[i] != *value {
                *value = QUALITY_STEPS[i];
                resp.mark_changed();
            }
        }
    }
    let cur = QUALITY_STEPS.iter().position(|q| q == value).unwrap_or(5);
    let max_available = available.and_then(|a| a.iter().copied().max());
    let painter = ui.painter();
    painter.line_segment([pos2(x0, line_y), pos2(x1, line_y)], Stroke::new(2.0, BORDER));
    painter.line_segment([pos2(x0, line_y), pos2(pos_x(cur), line_y)], Stroke::new(3.0, ACCENT));
    for (i, q) in QUALITY_STEPS.iter().enumerate() {
        let x = pos_x(i);
        let reachable = match (q.height(), max_available) {
            (Some(h), Some(max)) => h <= max,
            _ => true,
        };
        painter.circle_filled(pos2(x, line_y), 3.0, if i <= cur { ACCENT } else { BORDER });
        let color = if i == cur {
            TEXT
        } else if reachable {
            TEXT_DIM
        } else {
            TEXT_FAINT
        };
        painter.text(pos2(x, line_y + 22.0), egui::Align2::CENTER_CENTER, q.label(), FontId::proportional(12.5), color);
    }
    let knob = pos2(pos_x(cur), line_y);
    painter.circle_filled(knob, 7.5, ACCENT);
    painter.circle_stroke(knob, 7.5, Stroke::new(2.0, BG));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// A large two-line choice button (used in the "part of a playlist" chooser).
pub fn choice_button(ui: &mut Ui, icon: &str, title: &str, desc: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 66.0), Sense::click());
    let painter = ui.painter();
    let fill = if resp.hovered() { HOVER } else { SURFACE_HI };
    painter.rect_filled(rect, CornerRadius::same(8), fill);
    if resp.hovered() {
        painter.rect_stroke(rect, CornerRadius::same(8), Stroke::new(1.0, ACCENT_DIM), egui::StrokeKind::Inside);
    }
    painter.text(
        pos2(rect.left() + 26.0, rect.center().y),
        egui::Align2::CENTER_CENTER,
        icon,
        FontId::proportional(20.0),
        ACCENT_HOVER,
    );
    let text_x = rect.left() + 52.0;
    let max_w = rect.right() - text_x - 12.0;
    paint_line(painter, pos2(text_x, rect.top() + 13.0), title, FontId::proportional(15.5), TEXT, max_w);
    paint_line(painter, pos2(text_x, rect.top() + 37.0), desc, FontId::proportional(12.5), TEXT_DIM, max_w);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Menu,
    Folder,
    Pause,
    Play,
    Retry,
    Chevron,
}

/// Frameless icon button at a fixed position. Icons are painted (not font glyphs) so they
/// look the same whatever fonts the system has. Uses `interact`, so it doesn't affect layout.
pub fn icon_button(ui: &mut Ui, rect: Rect, id: egui::Id, icon: Icon, tooltip: &str) -> Response {
    let resp = ui.interact(rect, id, Sense::click());
    let painter = ui.painter();
    if resp.hovered() {
        painter.rect_filled(rect, CornerRadius::same(6), HOVER);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let c = if resp.hovered() { TEXT } else { TEXT_DIM };
    let o = rect.center();
    let p = |x: f32, y: f32| o + vec2(x, y);
    let stroke = Stroke::new(1.6, c);
    match icon {
        Icon::Menu => {
            for dy in [-5.0, 0.0, 5.0] {
                painter.circle_filled(p(0.0, dy), 1.8, c);
            }
        }
        Icon::Folder => {
            let body = Rect::from_min_max(p(-7.5, -3.5), p(7.5, 6.0));
            painter.rect_stroke(body, CornerRadius::same(2), stroke, egui::StrokeKind::Middle);
            painter.line(vec![p(-7.5, -3.5), p(-7.5, -6.0), p(-2.0, -6.0), p(0.0, -3.5)], stroke);
        }
        Icon::Pause => {
            for x in [-5.0, 1.5] {
                painter.rect_filled(Rect::from_min_size(p(x, -6.0), vec2(3.5, 12.0)), CornerRadius::same(1), c);
            }
        }
        Icon::Play => {
            painter.add(egui::Shape::convex_polygon(vec![p(-4.5, -6.5), p(-4.5, 6.5), p(6.5, 0.0)], c, Stroke::NONE));
        }
        Icon::Retry => {
            let r = 6.0;
            let (start, end) = (-0.35 * std::f32::consts::PI, 1.35 * std::f32::consts::PI);
            let pts: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let a = start + (end - start) * i as f32 / 24.0;
                    p(r * a.cos(), r * a.sin())
                })
                .collect();
            painter.line(pts, stroke);
            let tip = p(r * start.cos(), r * start.sin());
            painter.add(egui::Shape::convex_polygon(
                vec![tip + vec2(-4.0, -3.0), tip + vec2(3.0, -4.5), tip + vec2(1.5, 3.0)],
                c,
                Stroke::NONE,
            ));
        }
        Icon::Chevron => {
            painter.line(vec![p(-2.5, -6.0), p(3.5, 0.0), p(-2.5, 6.0)], Stroke::new(2.0, c));
        }
    }
    resp.on_hover_text(tooltip)
}
