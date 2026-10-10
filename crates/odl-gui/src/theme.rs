//! Colors and global style.

use eframe::egui::{self, vec2, Color32, CornerRadius, FontFamily, FontId, Margin, Stroke, TextStyle};

pub const ACCENT: Color32 = Color32::from_rgb(92, 184, 72);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(110, 201, 90);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(50, 92, 44);
pub const BG: Color32 = Color32::from_rgb(32, 33, 36);
pub const SURFACE: Color32 = Color32::from_rgb(40, 41, 45);
pub const SURFACE_HI: Color32 = Color32::from_rgb(49, 51, 56);
pub const HOVER: Color32 = Color32::from_rgb(57, 59, 65);
pub const BORDER: Color32 = Color32::from_rgb(64, 66, 72);
pub const FIELD: Color32 = Color32::from_rgb(27, 28, 31);
pub const TEXT: Color32 = Color32::from_rgb(230, 231, 234);
pub const TEXT_DIM: Color32 = Color32::from_rgb(152, 155, 162);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(106, 109, 116);
pub const DANGER: Color32 = Color32::from_rgb(234, 92, 82);
pub const WARN: Color32 = Color32::from_rgb(232, 176, 72);

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.text_styles = [
            (TextStyle::Heading, FontId::new(19.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
        ]
        .into();

        let s = &mut style.spacing;
        s.item_spacing = vec2(8.0, 6.0);
        s.button_padding = vec2(10.0, 5.0);
        s.interact_size = vec2(24.0, 28.0);
        s.menu_margin = Margin::same(6);
        s.window_margin = Margin::same(18);
        s.combo_width = 170.0;
        s.text_edit_width = 260.0;

        let v = &mut style.visuals;
        v.dark_mode = true;
        v.panel_fill = BG;
        v.window_fill = SURFACE;
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.window_corner_radius = CornerRadius::same(12);
        v.menu_corner_radius = CornerRadius::same(8);
        v.extreme_bg_color = FIELD;
        v.faint_bg_color = SURFACE;
        v.code_bg_color = SURFACE_HI;
        v.hyperlink_color = ACCENT_HOVER;
        v.selection.bg_fill = ACCENT_DIM;
        v.selection.stroke = Stroke::new(1.0, ACCENT_HOVER);
        v.warn_fg_color = WARN;
        v.error_fg_color = DANGER;

        let w = &mut v.widgets;
        for wv in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
            wv.corner_radius = CornerRadius::same(6);
        }
        w.noninteractive.bg_fill = SURFACE;
        w.noninteractive.weak_bg_fill = SURFACE;
        w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        w.inactive.bg_fill = SURFACE_HI;
        w.inactive.weak_bg_fill = SURFACE_HI;
        w.inactive.bg_stroke = Stroke::NONE;
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        w.hovered.bg_fill = HOVER;
        w.hovered.weak_bg_fill = HOVER;
        w.hovered.bg_stroke = Stroke::new(1.0, BORDER);
        w.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);
        w.active.bg_fill = ACCENT_DIM;
        w.active.weak_bg_fill = ACCENT_DIM;
        w.active.bg_stroke = Stroke::new(1.0, ACCENT);
        w.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);
        w.open.bg_fill = HOVER;
        w.open.weak_bg_fill = HOVER;
        w.open.fg_stroke = Stroke::new(1.0, TEXT);
    });
}

/// The big green call-to-action button.
pub fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.to_string()).color(Color32::WHITE).strong())
        .fill(ACCENT)
        .corner_radius(CornerRadius::same(6))
}

pub fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE_HI)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(14))
}

pub fn dialog_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(22))
}
