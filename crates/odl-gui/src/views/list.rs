//! The downloads list: tabs, rows, playlist drill-down and context menus.

use crate::app::{Action, OdlApp, Tab};
use crate::theme::*;
use crate::views::settings::{SettingsPage, SettingsView};
use crate::widgets::{icon_button, paint_line, Icon};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, RichText, Sense, Stroke, StrokeKind,
};
use odl_core::*;

const ROW_H: f32 = 70.0;
const THUMB: egui::Vec2 = egui::Vec2::new(104.0, 58.0);

impl OdlApp {
    pub fn ui_downloads(&mut self, ui: &mut egui::Ui) {
        let mut actions: Vec<Action> = Vec::new();

        let missing = self.missing_required_tools();
        if !missing.is_empty() {
            self.ui_missing_banner(ui, &missing);
        }

        if let Some(gid) = self.group_view.filter(|g| self.items.get(*g).is_some()) {
            self.ui_group_header(ui, gid, &mut actions);
        } else {
            self.group_view = None;
            self.ui_tabs(ui);
        }

        let ids = self.visible_ids();
        if ids.is_empty() {
            self.ui_empty(ui);
        } else {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_H, ids.len(), |ui, range| {
                for id in &ids[range] {
                    self.ui_row(ui, *id, &mut actions);
                }
            });
        }

        let ctx = ui.ctx().clone();
        self.apply_actions(&ctx, actions);
    }

    fn visible_ids(&self) -> Vec<ItemId> {
        let needle = self.search.trim().to_lowercase();
        let matches = |i: &DownloadItem| {
            needle.is_empty()
                || i.title.to_lowercase().contains(&needle)
                || i.uploader.as_deref().is_some_and(|u| u.to_lowercase().contains(&needle))
        };
        if let Some(gid) = self.group_view {
            return self
                .items
                .children_ids(gid)
                .into_iter()
                .filter(|id| self.items.get(*id).is_some_and(matches))
                .collect();
        }
        self.items
            .top_level()
            .filter(|i| match self.tab {
                Tab::All => true,
                Tab::Video => i.kind != ItemKind::Conversion && !i.is_audio(),
                Tab::Audio => i.is_audio(),
                Tab::Playlists => i.kind == ItemKind::Group,
                Tab::Converted => i.kind == ItemKind::Conversion,
            })
            .filter(|i| matches(i))
            .map(|i| i.id)
            .collect()
    }

    fn ui_missing_banner(&mut self, ui: &mut egui::Ui, missing: &[odl_core::tools::Tool]) {
        ui.add_space(6.0);
        card_frame().fill(Color32::from_rgb(58, 50, 30)).stroke(Stroke::new(1.0, WARN)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let names: Vec<&str> = missing.iter().map(|t| t.label()).collect();
                ui.label(RichText::new("⚠").size(18.0).color(WARN));
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("{} needed", names.join(" and "))).strong());
                    ui.label(
                        RichText::new("OpenDownloader uses these open-source tools to download and convert media.")
                            .small()
                            .color(TEXT_DIM),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let busy = missing.iter().any(|t| self.tool_state.get(t).is_some_and(|s| s.busy.is_some()));
                    if busy {
                        ui.spinner();
                        let status = missing
                            .iter()
                            .find_map(|t| self.tool_state.get(t).and_then(|s| s.busy.clone()))
                            .unwrap_or_default();
                        ui.label(RichText::new(status).small().color(TEXT_DIM));
                    } else {
                        if ui.add(primary_button("Install now")).clicked() {
                            self.install_missing_tools();
                        }
                        if ui.button("Details").clicked() {
                            self.settings_view = Some(SettingsView { page: SettingsPage::Components });
                        }
                    }
                });
            });
        });
        ui.add_space(4.0);
    }

    fn ui_tabs(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            for tab in Tab::ALL {
                let active = self.tab == tab;
                let text = RichText::new(tab.label()).size(14.5).color(if active { TEXT } else { TEXT_DIM });
                let r = ui.add(egui::Label::new(text).sense(Sense::click()));
                if active {
                    let y = r.rect.bottom() + 5.0;
                    ui.painter().line_segment([pos2(r.rect.left(), y), pos2(r.rect.right(), y)], Stroke::new(2.0, ACCENT));
                }
                if r.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if r.clicked() {
                    self.tab = tab;
                }
                ui.add_space(14.0);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(8.0);
                ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("🔍 Search").desired_width(190.0));
                let n = self.items.top_level().count();
                ui.label(RichText::new(format!("{n} item{}", if n == 1 { "" } else { "s" })).color(TEXT_DIM));
            });
        });
        ui.add_space(8.0);
        let r = ui.available_rect_before_wrap();
        ui.painter().line_segment([pos2(r.left() + 8.0, r.top()), pos2(r.right() - 8.0, r.top())], Stroke::new(1.0, SURFACE_HI));
        ui.add_space(4.0);
    }

    fn ui_group_header(&mut self, ui: &mut egui::Ui, gid: ItemId, actions: &mut Vec<Action>) {
        let Some(group) = self.items.get(gid) else { return };
        let s = self.items.summary(gid);
        let title = group.title.clone();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let back = ui.add(egui::Button::new(RichText::new("←").size(18.0)).corner_radius(CornerRadius::same(18)).min_size(vec2(36.0, 36.0)));
            if back.on_hover_text("Back").clicked() {
                self.group_view = None;
            }
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.label(RichText::new(&title).size(15.5).strong());
                let mut meta = format!("{} item{}", s.total, if s.total == 1 { "" } else { "s" });
                if s.duration > 0.0 {
                    meta.push_str(&format!(" · {}", format_duration(s.duration)));
                }
                if s.bytes > 0 {
                    meta.push_str(&format!(" · {}", format_bytes(s.bytes)));
                }
                ui.label(RichText::new(meta).small().color(TEXT_DIM));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(8.0);
                if s.running + s.queued > 0 {
                    if ui.button("⏸ Pause all").clicked() {
                        actions.push(Action::Pause(gid));
                    }
                } else if s.paused + s.failed > 0 && ui.button("▶ Resume all").clicked() {
                    actions.push(Action::Resume(gid));
                }
                if ui.button("📁").on_hover_text("Show in folder").clicked() {
                    actions.push(Action::ShowInFolder(gid));
                }
                ui.label(RichText::new(format!("downloaded {} of {}", s.completed, s.total)).color(TEXT_DIM));
            });
        });
        ui.add_space(8.0);
    }

    fn ui_empty(&self, ui: &mut egui::Ui) {
        ui.add_space(ui.available_height() * 0.22);
        ui.vertical_centered(|ui| {
            if !self.search.is_empty() {
                ui.label(RichText::new("Nothing matches your search").size(16.0).color(TEXT_DIM));
                return;
            }
            ui.label(RichText::new("⬇").size(54.0).color(ACCENT_DIM));
            ui.add_space(6.0);
            ui.label(RichText::new("Copy a link and press Paste Link").size(18.0).color(TEXT));
            ui.add_space(4.0);
            ui.label(
                RichText::new("Works with YouTube, Niconico, Bilibili and SoundCloud: videos, playlists, channels and more.")
                    .color(TEXT_DIM),
            );
            ui.label(RichText::new("Tip: press Ctrl+V anywhere in the window. Drop media files here to convert them.").small().color(TEXT_FAINT));
        });
    }

    fn ui_row(&self, ui: &mut egui::Ui, id: ItemId, actions: &mut Vec<Action>) {
        let Some(item) = self.items.get(id) else { return };
        let is_group = item.kind == ItemKind::Group;
        let summary = is_group.then(|| self.items.summary(id));
        let width = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click());
        if !ui.is_rect_visible(rect) {
            return;
        }
        let painter = ui.painter().clone();
        let card = rect.shrink2(vec2(6.0, 3.0));
        let selected = self.selected == Some(id);
        let fill = if resp.hovered() || resp.context_menu_opened() {
            SURFACE_HI
        } else if selected {
            SURFACE
        } else {
            Color32::TRANSPARENT
        };
        painter.rect_filled(card, CornerRadius::same(8), fill);
        if selected {
            painter.rect_stroke(card, CornerRadius::same(8), Stroke::new(1.0, ACCENT_DIM), StrokeKind::Inside);
        }

        // Thumbnail
        let thumb = Rect::from_min_size(pos2(card.left() + 10.0, card.center().y - THUMB.y / 2.0), THUMB);
        if is_group {
            for k in [2.0, 1.0] {
                let r = thumb.translate(vec2(3.0 * k, -3.0 * k)).shrink(2.0 * k);
                painter.rect_filled(r, CornerRadius::same(4), lerp_shade(k));
            }
        }
        painter.rect_filled(thumb, CornerRadius::same(5), SURFACE_HI);
        let thumb_url = item.thumbnail.clone().or_else(|| summary.as_ref().and_then(|s| s.thumbnail.clone()));
        match &thumb_url {
            Some(url) => egui::Image::new(url.as_str())
                .corner_radius(CornerRadius::same(5))
                .show_loading_spinner(false)
                .paint_at(ui, thumb),
            None => {
                let icon = match item.kind {
                    ItemKind::Conversion => "🔄",
                    _ if item.is_audio() => "🎵",
                    _ => "🎬",
                };
                painter.text(thumb.center(), Align2::CENTER_CENTER, icon, FontId::proportional(22.0), TEXT_FAINT);
            }
        }
        if let (Some(d), false) = (item.duration, is_group) {
            let label = format_duration(d);
            let galley = painter.layout_no_wrap(label, FontId::proportional(11.0), Color32::WHITE);
            let br = thumb.right_bottom() - vec2(4.0, 4.0);
            let bg = Rect::from_min_max(br - galley.size() - vec2(8.0, 2.0), br);
            painter.rect_filled(bg, CornerRadius::same(3), Color32::from_black_alpha(190));
            painter.galley(bg.min + vec2(4.0, 1.0), galley, Color32::WHITE);
        }
        if is_group {
            let n = summary.as_ref().map(|s| s.total).unwrap_or(0);
            let bg = Rect::from_min_size(thumb.left_top() + vec2(4.0, 4.0), vec2(28.0, 16.0));
            painter.rect_filled(bg, CornerRadius::same(3), Color32::from_black_alpha(190));
            painter.text(bg.center(), Align2::CENTER_CENTER, format!("☰{n}"), FontId::proportional(10.5), Color32::WHITE);
        }

        // Action buttons (right side)
        let btn = vec2(30.0, 30.0);
        let mut x = card.right() - 8.0 - btn.x;
        let y = card.center().y - btn.y / 2.0;
        let mut next_btn = || {
            let r = Rect::from_min_size(pos2(x, y), btn);
            x -= btn.x + 2.0;
            r
        };
        let menu_btn = icon_button(ui, next_btn(), egui::Id::new(("row-menu", id)), Icon::Menu, "More");
        egui::Popup::menu(&menu_btn).show(|ui| self.item_menu(ui, item, actions));
        if is_group {
            if icon_button(ui, next_btn(), egui::Id::new(("row-open", id)), Icon::Chevron, "Open playlist").clicked() {
                actions.push(Action::EnterGroup(id));
            }
        }
        match item.status {
            ItemStatus::Running | ItemStatus::Queued => {
                if icon_button(ui, next_btn(), egui::Id::new(("row-act", id)), Icon::Pause, "Pause").clicked() {
                    actions.push(Action::Pause(id));
                }
            }
            ItemStatus::Paused => {
                if icon_button(ui, next_btn(), egui::Id::new(("row-act", id)), Icon::Play, "Resume").clicked() {
                    actions.push(Action::Resume(id));
                }
            }
            ItemStatus::Failed => {
                if icon_button(ui, next_btn(), egui::Id::new(("row-act", id)), Icon::Retry, "Retry").clicked() {
                    actions.push(Action::Resume(id));
                }
            }
            ItemStatus::Completed => {
                if icon_button(ui, next_btn(), egui::Id::new(("row-act", id)), Icon::Folder, "Show in folder").clicked() {
                    actions.push(Action::ShowInFolder(id));
                }
            }
        }

        // Text
        let text_left = thumb.right() + 16.0;
        let text_w = (x + btn.x - 8.0 - text_left).max(40.0);
        let title = match (item.index, item.parent) {
            (Some(i), Some(_)) => format!("{i}. {}", item.title),
            _ => item.title.clone(),
        };
        paint_line(&painter, pos2(text_left, card.top() + 13.0), &title, FontId::proportional(14.5), TEXT, text_w);
        let meta_pos = pos2(text_left, card.top() + 36.0);
        let small = FontId::proportional(12.5);

        let (status, fraction, meta, meta_color) = match &summary {
            Some(s) => {
                let status = s.status();
                let meta = match status {
                    ItemStatus::Running | ItemStatus::Queued => format!(
                        "{:.0}% downloaded · {} of {} items{}",
                        s.fraction * 100.0,
                        s.completed,
                        s.total,
                        if s.speed > 0.0 { format!(" · {}/s", format_bytes(s.speed as u64)) } else { String::new() }
                    ),
                    _ => {
                        let mut m = format!(
                            "{} {} · {}",
                            s.total,
                            if item.is_audio() { "tracks" } else { "videos" },
                            format_bytes(s.bytes)
                        );
                        if s.failed > 0 {
                            m.push_str(&format!(" · {} failed", s.failed));
                        }
                        if s.paused > 0 {
                            m.push_str(&format!(" · {} paused", s.paused));
                        }
                        m
                    }
                };
                let color = if s.failed > 0 && status == ItemStatus::Failed { DANGER } else { TEXT_DIM };
                (status, Some(s.fraction), meta, color)
            }
            None => {
                let (meta, color) = item_meta(item);
                (item.status, item.progress.fraction, meta, color)
            }
        };
        paint_line(&painter, meta_pos, &meta, small, meta_color, text_w);
        if matches!(status, ItemStatus::Running | ItemStatus::Queued | ItemStatus::Paused) {
            let bar = Rect::from_min_size(pos2(text_left, card.bottom() - 13.0), vec2(text_w, 4.0));
            painter.rect_filled(bar, CornerRadius::same(2), SURFACE_HI);
            let f = fraction.unwrap_or(0.0).clamp(0.0, 1.0);
            if f > 0.0 {
                let filled = Rect::from_min_size(bar.min, vec2(bar.width() * f, bar.height()));
                let color = if status == ItemStatus::Paused { WARN } else { ACCENT };
                painter.rect_filled(filled, CornerRadius::same(2), color);
            } else if status == ItemStatus::Running {
                // Indeterminate: a sliding segment.
                let t = (ui.input(|i| i.time) as f32 * 0.6).fract();
                let seg_w = bar.width() * 0.25;
                let x0 = bar.left() + (bar.width() + seg_w) * t - seg_w;
                let seg = Rect::from_min_max(pos2(x0.max(bar.left()), bar.top()), pos2((x0 + seg_w).min(bar.right()), bar.bottom()));
                painter.rect_filled(seg, CornerRadius::same(2), ACCENT_DIM);
                ui.ctx().request_repaint();
            }
        }

        // Row interactions (buttons above take precedence)
        if resp.clicked() {
            if is_group {
                actions.push(Action::EnterGroup(id));
            } else {
                actions.push(Action::Select(id));
            }
        }
        if resp.double_clicked() && item.status == ItemStatus::Completed && !is_group {
            actions.push(Action::OpenFile(id));
        }
        if let Some(err) = item.error.as_ref().filter(|_| item.status == ItemStatus::Failed) {
            let resp = resp.clone().on_hover_text(err.as_str());
            resp.context_menu(|ui| self.item_menu(ui, item, actions));
        } else {
            resp.context_menu(|ui| self.item_menu(ui, item, actions));
        }
    }

    fn item_menu(&self, ui: &mut egui::Ui, item: &DownloadItem, actions: &mut Vec<Action>) {
        ui.set_min_width(220.0);
        let id = item.id;
        fn act(ui: &mut egui::Ui, actions: &mut Vec<Action>, label: &str, shortcut: Option<&str>, action: Action) {
            let mut b = egui::Button::new(label);
            if let Some(s) = shortcut {
                b = b.shortcut_text(s);
            }
            if ui.add(b).clicked() {
                actions.push(action);
                ui.close();
            }
        }
        let has_file = item.file_path.as_ref().is_some_and(|p| p.exists());
        let is_group = item.kind == ItemKind::Group;

        if is_group {
            act(ui, actions, "Open Playlist", None, Action::EnterGroup(id));
        } else if item.status == ItemStatus::Completed && has_file {
            act(ui, actions, "Open", None, Action::OpenFile(id));
        }
        act(ui, actions, "Show in Folder", None, Action::ShowInFolder(id));
        if !item.url.is_empty() {
            act(ui, actions, "Copy Link Address", None, Action::CopyLink(id));
            act(ui, actions, "Open Link in Browser", None, Action::OpenLink(id));
        }
        act(ui, actions, "Paste Link", Some("Ctrl+V"), Action::Paste);
        ui.separator();

        match item.status {
            ItemStatus::Running | ItemStatus::Queued => act(ui, actions, "Pause", None, Action::Pause(id)),
            ItemStatus::Paused => act(ui, actions, "Resume", None, Action::Resume(id)),
            ItemStatus::Failed => {
                act(ui, actions, "Retry", None, Action::Resume(id));
                if item.error.is_some() {
                    act(ui, actions, "Copy Error Message", None, Action::CopyError(id));
                }
            }
            ItemStatus::Completed => {}
        }
        if item.kind == ItemKind::Media && !item.url.is_empty() {
            act(ui, actions, "Download Again…", None, Action::DownloadAgain(id));
        }
        if has_file && !is_group {
            ui.menu_button("Media Tools", |ui| {
                ui.set_min_width(180.0);
                for target in ConvertTarget::ALL {
                    if item.is_audio() && !target.is_audio() {
                        continue;
                    }
                    if ui.button(format!("Convert to {}", target.label())).clicked() {
                        actions.push(Action::Convert(id, target));
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("More Options…").clicked() {
                    actions.push(Action::ConvertDialog(id));
                    ui.close();
                }
            });
        }
        ui.separator();
        act(ui, actions, "Remove from List", Some("Del"), Action::Remove(id));
        let unfinished = if is_group {
            let s = self.items.summary(id);
            s.completed < s.total
        } else {
            item.kind == ItemKind::Media && item.status != ItemStatus::Completed
        };
        if unfinished {
            let label = if is_group { "Remove and Delete Partial Downloads…" } else { "Remove and Delete Partial Download…" };
            act(ui, actions, label, None, Action::AskDeletePartial(id));
        }
        if has_file && !is_group {
            act(ui, actions, "Delete File…", None, Action::AskDeleteFile(id));
        }
        act(ui, actions, "Remove All Completed", None, Action::ClearCompleted);
        act(ui, actions, "Clear List…", None, Action::AskClearList);
    }
}

fn lerp_shade(k: f32) -> Color32 {
    if k > 1.5 {
        Color32::from_rgb(44, 46, 50)
    } else {
        Color32::from_rgb(54, 56, 61)
    }
}

/// Second line of a row, e.g. `05:12 · 45.2 MB · MP4 · 1080p · 30fps · M!LK`.
fn item_meta(item: &DownloadItem) -> (String, Color32) {
    let p = &item.progress;
    match item.status {
        ItemStatus::Queued => ("Waiting…".into(), TEXT_DIM),
        ItemStatus::Running => {
            if let Some(stage) = &p.stage {
                let pct = p.fraction.map(|f| format!(" · {:.0}%", f * 100.0)).unwrap_or_default();
                return (format!("{stage}{pct}"), ACCENT_HOVER);
            }
            let mut parts = Vec::new();
            match p.fraction {
                Some(f) => parts.push(format!("{:.0}% downloaded", f * 100.0)),
                None => parts.push("Starting…".into()),
            }
            if p.downloaded > 0 {
                match p.total {
                    Some(t) => parts.push(format!("{} of {}", format_bytes(p.downloaded), format_bytes(t))),
                    None => parts.push(format_bytes(p.downloaded)),
                }
            }
            if let Some(s) = p.speed.filter(|s| *s > 0.0) {
                parts.push(format!("{}/s", format_bytes(s as u64)));
            }
            if let Some(eta) = p.eta {
                parts.push(format_eta(eta));
            }
            (parts.join(" · "), TEXT)
        }
        ItemStatus::Paused => {
            let pct = p.fraction.map(|f| format!(" · {:.0}%", f * 100.0)).unwrap_or_default();
            (format!("Paused{pct}"), WARN)
        }
        ItemStatus::Failed => {
            let first = item.error.as_deref().and_then(|e| e.lines().next()).unwrap_or("Failed");
            (format!("Failed: {first}"), DANGER)
        }
        ItemStatus::Completed => {
            let mut parts = Vec::new();
            if let Some(d) = item.duration {
                parts.push(format_duration(d));
            }
            if let Some(s) = item.file_size {
                parts.push(format_bytes(s));
            }
            if let Some(ext) = &item.ext {
                parts.push(ext.to_uppercase());
            }
            if let Some(h) = item.height {
                parts.push(format!("{h}p"));
            }
            if let Some(fps) = item.fps.filter(|f| *f > 0.0) {
                parts.push(format!("{}fps", fps.round()));
            }
            if let Some(u) = &item.uploader {
                parts.push(u.clone());
            }
            if let Some(note) = &item.note {
                parts.push(note.clone());
            }
            if item.file_path.as_ref().is_some_and(|f| !f.exists()) {
                parts.push("file missing".into());
            }
            (parts.join(" · "), TEXT_DIM)
        }
    }
}
