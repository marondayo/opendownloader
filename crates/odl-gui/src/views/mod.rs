//! Top-level layout: menu bar, toolbar, status bar, dialogs and toasts.

pub mod add;
pub mod convert;
pub mod list;
pub mod settings;
pub mod subs;

use crate::app::{Action, Confirm, OdlApp, ToastKind};
use crate::theme::*;
use crate::views::settings::{SettingsPage, SettingsView};
use crate::widgets;
use eframe::egui::{self, vec2, Align, Align2, CornerRadius, Frame, Id, Layout, Margin, RichText, Stroke};
use odl_core::format_bytes;

pub const REPO_URL: &str = "https://github.com/marondayo/opendownloader";

impl OdlApp {
    pub fn ui_root(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        egui::Panel::top("menu")
            .frame(Frame::new().fill(BG).inner_margin(Margin::symmetric(8, 2)))
            .show(ui, |ui| self.ui_menu(ui));
        egui::Panel::top("toolbar")
            .frame(Frame::new().fill(BG).inner_margin(Margin { left: 16, right: 16, top: 10, bottom: 8 }))
            .show(ui, |ui| self.ui_toolbar(ui));
        egui::Panel::bottom("status")
            .frame(Frame::new().fill(SURFACE).inner_margin(Margin::symmetric(16, 6)))
            .show(ui, |ui| self.ui_statusbar(ui));
        egui::CentralPanel::no_frame().show(ui, |ui| {
            Frame::new().fill(BG).inner_margin(Margin { left: 8, right: 8, top: 0, bottom: 4 }).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                self.ui_downloads(ui);
            });
        });

        self.ui_add_flow(&ctx);
        self.ui_settings(&ctx);
        self.ui_convert(&ctx);
        self.ui_confirm(&ctx);
        self.ui_about(&ctx);
        self.ui_toasts(&ctx);
        self.handle_shortcuts(&ctx);
    }

    fn modal_open(&self) -> bool {
        self.add.is_some()
            || self.settings_view.is_some()
            || self.convert_view.is_some()
            || self.confirm.is_some()
            || self.about_open
    }

    fn ui_menu(&mut self, ui: &mut egui::Ui) {
        let mut actions = Vec::new();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.add(egui::Button::new("Paste Link").shortcut_text("Ctrl+V")).clicked() {
                    actions.push(Action::Paste);
                }
                if ui.button("Convert Media Files…").clicked() {
                    self.convert_view = Some(Default::default());
                }
                if ui.button("Open Downloads Folder").clicked() {
                    let dir = self.settings.download_dir.clone();
                    let _ = std::fs::create_dir_all(&dir);
                    crate::platform::open_path(&dir);
                }
                ui.separator();
                if ui.button("Resume All").clicked() {
                    self.resume_all();
                }
                if ui.button("Pause All").clicked() {
                    self.pause_all();
                }
                if ui.button("Remove Completed").clicked() {
                    actions.push(Action::ClearCompleted);
                }
                if ui.button("Clear List…").clicked() {
                    actions.push(Action::AskClearList);
                }
                ui.separator();
                if ui.add(egui::Button::new("Quit").shortcut_text("Ctrl+Q")).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Tools", |ui| {
                if ui.button("Media Converter…").clicked() {
                    self.convert_view = Some(Default::default());
                }
                if ui.button("Accounts…").clicked() {
                    self.settings_view = Some(SettingsView { page: SettingsPage::Accounts });
                }
                if ui.button("Update yt-dlp").clicked() {
                    self.update_ytdlp();
                }
                ui.separator();
                if ui.add(egui::Button::new("Settings…").shortcut_text("Ctrl+,")).clicked() {
                    self.settings_view = Some(SettingsView::default());
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Supported Sites").clicked() {
                    self.toast(
                        ToastKind::Info,
                        "YouTube (videos, playlists, channels), Niconico (videos, mylists, series), \
                         Bilibili (videos, multi-part, bangumi, favorites), SoundCloud (tracks, sets, profiles)",
                    );
                }
                if ui.button("Report an Issue").clicked() {
                    crate::platform::open_url(&format!("{REPO_URL}/issues"));
                }
                ui.separator();
                if ui.button("About OpenDownloader").clicked() {
                    self.about_open = true;
                }
            });
        });
        let ctx = ui.ctx().clone();
        self.apply_actions(&ctx, actions);
    }

    fn ui_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let paste = ui.add(crate::theme::primary_button("⬇  Paste Link").min_size(vec2(176.0, 40.0)));
            if paste.clicked() {
                self.request_paste(ui.ctx());
            }
            paste.on_hover_text("Download the link in your clipboard (Ctrl+V)");
            ui.add_space(14.0);
            if widgets::toggle(ui, &mut self.settings.smart_mode).changed() {
                self.settings_changed();
            }
            let text = if self.settings.smart_mode {
                format!("Smart Mode on: one-click downloads as {}", self.settings.smart_preset.summary())
            } else {
                "Enable Smart Mode to download links in one click".to_string()
            };
            ui.label(RichText::new(text).color(TEXT_DIM));

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let gear = ui.add(egui::Button::new(RichText::new("⚙").size(20.0)).frame(false)).on_hover_text("Settings");
                if gear.clicked() {
                    self.settings_view = Some(SettingsView::default());
                }
                let conv = ui
                    .add(egui::Button::new(RichText::new("🔄").size(18.0)).frame(false))
                    .on_hover_text("Media converter");
                if conv.clicked() {
                    self.convert_view = Some(Default::default());
                }
                let acct = ui.add(egui::Button::new(RichText::new("👤").size(18.0)).frame(false)).on_hover_text("Accounts");
                if acct.clicked() {
                    self.settings_view = Some(SettingsView { page: SettingsPage::Accounts });
                }
            });
        });
    }

    fn ui_statusbar(&mut self, ui: &mut egui::Ui) {
        let dir = self.settings.download_dir.clone();
        let free = self.free_space_cached(&dir);
        let reserve = self.keep_free_bytes();
        ui.horizontal(|ui| {
            let (running, queued, speed) = self.items.running_count();
            let text = match (running, queued) {
                (0, 0) => "Ready".to_string(),
                (r, 0) => format!("{r} downloading · {}/s", format_bytes(speed as u64)),
                (r, q) => format!("{r} downloading · {q} waiting · {}/s", format_bytes(speed as u64)),
            };
            ui.label(RichText::new(text).small().color(TEXT_DIM));
            if let Some(free) = free {
                // Warn once less than twice the keep-free limit (or 5 GB) is left.
                let low = free < (reserve * 2).max(5 << 30);
                let color = if reserve > 0 && free < reserve { DANGER } else if low { WARN } else { TEXT_FAINT };
                ui.label(RichText::new("·").small().color(TEXT_FAINT));
                ui.label(RichText::new(format!("{} free", format_bytes(free))).small().color(color))
                    .on_hover_text(format!("Free space in {}", dir.display()));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let v = self
                    .tool_state
                    .get(&odl_core::tools::Tool::YtDlp)
                    .and_then(|s| s.version.clone())
                    .map(|v| format!("yt-dlp {v}"))
                    .unwrap_or_else(|| "yt-dlp not installed".into());
                let r = ui.add(egui::Label::new(RichText::new(v).small().color(TEXT_FAINT)).sense(egui::Sense::click()));
                if r.on_hover_text("Components").clicked() {
                    self.settings_view = Some(SettingsView { page: SettingsPage::Components });
                }
                ui.label(RichText::new(format!("OpenDownloader {}", env!("CARGO_PKG_VERSION"))).small().color(TEXT_FAINT));
            });
        });
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let text_focus = ctx.text_edit_focused();
        let (paste, delete, quit, settings, dropped) = ctx.input(|i| {
            let paste = i.events.iter().find_map(|e| match e {
                egui::Event::Paste(t) => Some(t.clone()),
                _ => None,
            });
            (
                paste,
                i.key_pressed(egui::Key::Delete),
                i.modifiers.command && i.key_pressed(egui::Key::Q),
                i.modifiers.command && i.key_pressed(egui::Key::Comma),
                i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect::<Vec<_>>(),
            )
        });
        if let Some(text) = paste {
            let requested = self.paste_requested.take().is_some();
            // Stray pastes of non-link text (e.g. clipboard sync) are ignored quietly;
            // the Paste Link button always reports what went wrong.
            if requested || (!text_focus && !self.modal_open() && odl_core::parse_link(&text).is_ok()) {
                self.begin_link(&text);
            }
        }
        if quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if settings && self.settings_view.is_none() {
            self.settings_view = Some(SettingsView::default());
        }
        if delete && !text_focus && !self.modal_open() {
            if let Some(id) = self.selected {
                self.remove(id);
            }
        }
        if !dropped.is_empty() {
            let view = self.convert_view.get_or_insert_with(Default::default);
            for p in dropped {
                if p.is_file() && !view.files.contains(&p) {
                    view.files.push(p);
                }
            }
        }
    }

    fn ui_toasts(&mut self, ctx: &egui::Context) {
        if self.toasts.is_empty() && self.clip_offer.is_none() {
            return;
        }
        let mut dismiss = None;
        let (mut take_offer, mut drop_offer) = (false, false);
        egui::Area::new(Id::new("toasts"))
            .anchor(Align2::RIGHT_BOTTOM, vec2(-16.0, -40.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_max_width(420.0);
                if let Some(offer) = &self.clip_offer {
                    Frame::new()
                        .fill(SURFACE_HI)
                        .stroke(Stroke::new(1.0, ACCENT))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            ui.set_width(380.0);
                            ui.label(RichText::new(format!("📋 {} link copied", offer.link.site.name())).strong());
                            ui.add(egui::Label::new(RichText::new(&offer.link.url).small().color(TEXT_DIM)).truncate());
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                take_offer = ui.add(primary_button("Download")).clicked();
                                drop_offer = ui.button("Dismiss").clicked();
                            });
                        });
                    ui.add_space(6.0);
                }
                for (i, t) in self.toasts.iter().enumerate() {
                    let color = match t.kind {
                        ToastKind::Info => BORDER,
                        ToastKind::Success => ACCENT,
                        ToastKind::Error => DANGER,
                    };
                    let r = Frame::new()
                        .fill(SURFACE_HI)
                        .stroke(Stroke::new(1.0, color))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(RichText::new(&t.text).color(TEXT)).wrap());
                        })
                        .response
                        .interact(egui::Sense::click());
                    if r.clicked() {
                        dismiss = Some(i);
                    }
                    ui.add_space(6.0);
                }
            });
        if let Some(i) = dismiss {
            self.toasts.remove(i);
        }
        if take_offer {
            if let Some(offer) = self.clip_offer.take() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                self.begin_link(&offer.text);
            }
        } else if drop_offer {
            self.clip_offer = None;
        }
    }

    fn ui_confirm(&mut self, ctx: &egui::Context) {
        let Some(confirm) = &self.confirm else { return };
        let (title, body, ok) = match confirm {
            Confirm::DeleteFile(id) => {
                let name = self.items.get(*id).map(|i| i.title.clone()).unwrap_or_default();
                ("Delete file?", format!("“{name}” will be deleted from disk and removed from the list."), "Delete")
            }
            Confirm::DeletePartial(id) => {
                let name = self.items.get(*id).map(|i| i.title.clone()).unwrap_or_default();
                let size = self.partial_size(*id);
                let amount = if size > 0 { format!("the {} downloaded so far", format_bytes(size)) } else { "its partial files".into() };
                (
                    "Delete partial download?",
                    format!("“{name}” is stopped, removed from the list, and {amount} is deleted. It can't be resumed afterwards."),
                    "Delete",
                )
            }
            Confirm::ClearList => (
                "Clear the list?",
                "All items are removed from the list. Downloaded files stay on disk; running downloads are stopped."
                    .to_string(),
                "Clear List",
            ),
            Confirm::Quit => (
                "Downloads in progress",
                "Quit anyway? Unfinished downloads are paused and can be resumed next time.".to_string(),
                "Quit",
            ),
        };
        let mut result: Option<bool> = None;
        let modal = egui::Modal::new(Id::new("confirm")).frame(dialog_frame()).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading(title);
            ui.add_space(6.0);
            ui.add(egui::Label::new(RichText::new(body).color(TEXT_DIM)).wrap());
            ui.add_space(16.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let ok_btn = egui::Button::new(RichText::new(ok).color(egui::Color32::WHITE)).fill(DANGER);
                if ui.add(ok_btn).clicked() {
                    result = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    result = Some(false);
                }
            });
        });
        if modal.should_close() && result.is_none() {
            result = Some(false);
        }
        let Some(yes) = result else { return };
        let confirm = self.confirm.take();
        if !yes {
            return;
        }
        match confirm {
            Some(Confirm::DeleteFile(id)) => self.delete_file(id),
            Some(Confirm::DeletePartial(id)) => self.delete_partial(id),
            Some(Confirm::ClearList) => self.clear_list(),
            Some(Confirm::Quit) => self.quit(ctx),
            None => {}
        }
    }

    fn ui_about(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        let modal = egui::Modal::new(Id::new("about")).frame(dialog_frame()).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("⬇").size(40.0).color(ACCENT));
                ui.label(RichText::new("OpenDownloader").size(22.0).strong());
                ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(TEXT_DIM));
                ui.add_space(8.0);
                ui.add(
                    egui::Label::new(
                        "A free and open-source video & audio downloader for YouTube, Niconico, Bilibili and SoundCloud.",
                    )
                    .wrap(),
                );
                ui.add_space(8.0);
                ui.label(RichText::new("Licensed under the GNU GPL v3 or later.").small().color(TEXT_DIM));
                ui.label(
                    RichText::new("Powered by yt-dlp, FFmpeg, Deno and egui. Thank you to their authors.")
                        .small()
                        .color(TEXT_DIM),
                );
                ui.add_space(8.0);
                ui.hyperlink_to("Source code", REPO_URL);
                ui.add_space(8.0);
                if ui.button("Close").clicked() {
                    self.about_open = false;
                }
            });
        });
        if modal.should_close() {
            self.about_open = false;
        }
    }
}

