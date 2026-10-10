//! The Subscriptions tab.

use crate::app::{OdlApp, ToastKind};
use crate::theme::*;
use crate::widgets::{icon_button, paint_line, Icon};
use eframe::egui::{self, pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, RichText, Sense};
use odl_core::{sanitize_filename, Subscription};
use std::path::PathBuf;

const ROW_H: f32 = 70.0;

enum SubAction {
    Check(u64),
    CheckAll,
    SetEnabled(u64, bool),
    OpenFolder(u64),
    CopyLink(u64),
    OpenLink(u64),
    Unsubscribe(u64),
}

fn ago(t: chrono::DateTime<chrono::Utc>) -> String {
    let mins = (chrono::Utc::now() - t).num_minutes().max(0);
    match mins {
        0 => "just now".into(),
        1..=59 => format!("{mins} min ago"),
        60..=1439 => format!("{} h ago", mins / 60),
        _ => format!("{} days ago", mins / 1440),
    }
}

impl OdlApp {
    fn sub_folder(&self, s: &Subscription) -> PathBuf {
        if self.settings.playlist_subdirs {
            s.options.output_dir.join(sanitize_filename(&s.title))
        } else {
            s.options.output_dir.clone()
        }
    }

    pub fn ui_subscriptions(&mut self, ui: &mut egui::Ui) {
        let mut actions = Vec::new();
        if self.subs.is_empty() {
            ui.add_space(ui.available_height() * 0.22);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("🔔").size(46.0).color(ACCENT_DIM));
                ui.add_space(6.0);
                ui.label(RichText::new("No subscriptions yet").size(18.0));
                ui.add_space(4.0);
                ui.label(
                    RichText::new("When you download a playlist, channel, mylist or favorites list, tick “🔔 Subscribe”.")
                        .color(TEXT_DIM),
                );
                ui.label(
                    RichText::new("New videos are then downloaded automatically with the same settings.").color(TEXT_DIM),
                );
            });
            return;
        }

        ui.horizontal(|ui| {
            ui.add_space(12.0);
            let every = match self.settings.subscription_check_hours {
                0 => "automatic checks off".to_string(),
                1 => "checked every hour".to_string(),
                h => format!("checked every {h} hours"),
            };
            ui.label(RichText::new(format!("{} subscription{} · {every}", self.subs.len(), if self.subs.len() == 1 { "" } else { "s" })).color(TEXT_DIM));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(12.0);
                if ui.button("⟳ Check all now").clicked() {
                    actions.push(SubAction::CheckAll);
                }
            });
        });
        ui.add_space(6.0);

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for s in &self.subs {
                self.ui_sub_row(ui, s, &mut actions);
            }
        });

        for a in actions {
            match a {
                SubAction::Check(id) => self.check_subscription(id),
                SubAction::CheckAll => self.check_all_subscriptions(),
                SubAction::SetEnabled(id, on) => self.set_subscription_enabled(id, on),
                SubAction::OpenFolder(id) => {
                    if let Some(dir) = self.subs.iter().find(|s| s.id == id).map(|s| self.sub_folder(s)) {
                        if dir.exists() {
                            crate::platform::open_path(&dir);
                        } else {
                            self.toast(ToastKind::Info, "Nothing downloaded into this folder yet.");
                        }
                    }
                }
                SubAction::CopyLink(id) => {
                    if let Some(url) = self.subs.iter().find(|s| s.id == id).map(|s| s.url.clone()) {
                        let ctx = ui.ctx().clone();
                        self.copy_text(&ctx, url);
                        self.toast(ToastKind::Info, "Link copied");
                    }
                }
                SubAction::OpenLink(id) => {
                    if let Some(s) = self.subs.iter().find(|s| s.id == id) {
                        crate::platform::open_url(&s.url);
                    }
                }
                SubAction::Unsubscribe(id) => {
                    let title = self.subs.iter().find(|s| s.id == id).map(|s| s.title.clone()).unwrap_or_default();
                    self.unsubscribe(id);
                    self.toast(ToastKind::Info, format!("Unsubscribed from “{title}”. Downloaded files are kept."));
                }
            }
        }
    }

    fn ui_sub_row(&self, ui: &mut egui::Ui, s: &Subscription, actions: &mut Vec<SubAction>) {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
        let painter = ui.painter().clone();
        let card = rect.shrink2(vec2(6.0, 3.0));
        if resp.hovered() || resp.context_menu_opened() {
            painter.rect_filled(card, CornerRadius::same(8), SURFACE_HI);
        }
        let thumb = Rect::from_min_size(pos2(card.left() + 10.0, card.center().y - 29.0), vec2(104.0, 58.0));
        painter.rect_filled(thumb, CornerRadius::same(5), SURFACE_HI);
        match &s.thumbnail {
            Some(url) => egui::Image::new(url.as_str())
                .corner_radius(CornerRadius::same(5))
                .show_loading_spinner(false)
                .tint(if s.enabled { Color32::WHITE } else { Color32::from_gray(120) })
                .paint_at(ui, thumb),
            None => {
                painter.text(thumb.center(), Align2::CENTER_CENTER, "🔔", FontId::proportional(22.0), TEXT_FAINT);
            }
        }

        let id = s.id;
        let btn = vec2(30.0, 30.0);
        let y = card.center().y - btn.y / 2.0;
        let menu_rect = Rect::from_min_size(pos2(card.right() - 8.0 - btn.x, y), btn);
        let toggle_rect = menu_rect.translate(vec2(-(btn.x + 2.0), 0.0));
        let check_rect = Rect::from_min_size(pos2(toggle_rect.left() - 100.0, y), vec2(96.0, btn.y));
        let menu_btn = icon_button(ui, menu_rect, egui::Id::new(("sub-menu", id)), Icon::Menu, "More");
        let (icon, tip) = if s.enabled { (Icon::Pause, "Pause subscription") } else { (Icon::Play, "Resume subscription") };
        if icon_button(ui, toggle_rect, egui::Id::new(("sub-toggle", id)), icon, tip).clicked() {
            actions.push(SubAction::SetEnabled(id, !s.enabled));
        }
        let checking = self.sub_checking == Some(id);
        if !checking && ui.put(check_rect, egui::Button::new("Check now")).clicked() {
            actions.push(SubAction::Check(id));
        }

        let text_left = thumb.right() + 16.0;
        let text_w = (check_rect.left() - 8.0 - text_left).max(40.0);
        paint_line(&painter, pos2(text_left, card.top() + 13.0), &s.title, FontId::proportional(14.5), if s.enabled { TEXT } else { TEXT_DIM }, text_w);
        let (meta, color) = if checking {
            ("Checking for new videos…".to_string(), ACCENT_HOVER)
        } else if let Some(err) = &s.last_error {
            (format!("Check failed: {}", err.lines().next().unwrap_or("")), DANGER)
        } else {
            let mut parts = vec![s.site.name().to_string()];
            if !s.enabled {
                parts.push("Paused".into());
            }
            if let Some(t) = s.last_checked {
                parts.push(format!("checked {}", ago(t)));
            }
            if s.last_new > 0 {
                parts.push(format!("{} new last time", s.last_new));
            }
            parts.push(format!("{} downloaded", s.total_downloaded));
            (parts.join(" · "), if s.enabled { TEXT_DIM } else { WARN })
        };
        paint_line(&painter, pos2(text_left, card.top() + 36.0), &meta, FontId::proportional(12.5), color, text_w);
        if checking {
            let r = Rect::from_center_size(check_rect.center(), vec2(18.0, 18.0));
            ui.put(r, egui::Spinner::new().size(16.0).color(ACCENT));
        }

        let menu = |ui: &mut egui::Ui, actions: &mut Vec<SubAction>| {
            ui.set_min_width(200.0);
            let mut item = |ui: &mut egui::Ui, label: &str, a: SubAction| {
                if ui.button(label).clicked() {
                    actions.push(a);
                    ui.close();
                }
            };
            item(ui, "Check Now", SubAction::Check(id));
            item(ui, if s.enabled { "Pause" } else { "Resume" }, SubAction::SetEnabled(id, !s.enabled));
            item(ui, "Open Folder", SubAction::OpenFolder(id));
            item(ui, "Copy Link Address", SubAction::CopyLink(id));
            item(ui, "Open Link in Browser", SubAction::OpenLink(id));
            ui.separator();
            item(ui, "Unsubscribe", SubAction::Unsubscribe(id));
        };
        egui::Popup::menu(&menu_btn).show(|ui| menu(ui, actions));
        resp.context_menu(|ui| menu(ui, actions));
    }
}
