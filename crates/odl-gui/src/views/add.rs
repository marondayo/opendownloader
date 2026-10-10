//! Adding a link: playlist chooser, "analyzing" spinner and the download options dialog.

use crate::app::{OdlApp, PickTarget};
use crate::theme::*;
use crate::widgets::{self, choice_button, field_label, paint_line};
use eframe::egui::{self, pos2, vec2, Align, CornerRadius, FontId, Id, Layout, Rect, RichText, Sense};
use odl_core::*;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq)]
pub enum Preselect {
    All,
    Only(String),
}

pub enum AddFlow {
    /// The link is a video inside a playlist: ask what to download.
    Choose(Link),
    Probing { req: u64, link: Link, url: String, preselect: Preselect, started: Instant },
    Dialog(Box<AddDialog>),
    Error(String),
}

pub struct AddDialog {
    pub link: Link,
    pub probe: ProbeResult,
    pub opts: DownloadOptions,
    pub checked: Vec<bool>,
    pub filter: String,
}

impl AddDialog {
    pub fn new(link: Link, probe: ProbeResult, preselect: Preselect, settings: &Settings) -> Self {
        let mut opts = settings.dialog_defaults();
        let video_possible = link.site.has_video()
            && match &probe {
                ProbeResult::Single(info) => info.has_video(),
                ProbeResult::Playlist(_) => true,
            };
        if !video_possible {
            opts.media = MediaKind::Audio;
        }
        if opts.container == VideoContainer::Webm && link.site != Site::YouTube {
            opts.container = VideoContainer::Mp4;
        }
        let checked = match &probe {
            ProbeResult::Playlist(p) => p
                .entries
                .iter()
                .map(|e| match &preselect {
                    Preselect::All => true,
                    Preselect::Only(id) => &e.id == id,
                })
                .collect(),
            ProbeResult::Single(_) => Vec::new(),
        };
        Self { link, probe, opts, checked, filter: String::new() }
    }

    fn selected_count(&self) -> usize {
        self.checked.iter().filter(|c| **c).count()
    }
}

enum DialogOutcome {
    Keep,
    Close,
    Download,
}

impl OdlApp {
    pub fn ui_add_flow(&mut self, ctx: &egui::Context) {
        let Some(flow) = self.add.take() else { return };
        self.add = match flow {
            AddFlow::Choose(link) => self.ui_choose(ctx, link),
            AddFlow::Probing { req, link, url, preselect, started } => {
                let mut cancel = false;
                let modal = egui::Modal::new(Id::new("probing")).frame(dialog_frame()).show(ctx, |ui| {
                    ui.set_width(420.0);
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(22.0).color(ACCENT));
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Analyzing link…").size(16.0).strong());
                            ui.add(egui::Label::new(RichText::new(&url).small().color(TEXT_DIM)).truncate());
                        });
                    });
                    if started.elapsed().as_secs() >= 6 {
                        ui.add_space(6.0);
                        widgets::hint(ui, "Large playlists and channels can take a while to list.");
                    }
                    ui.add_space(10.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
                if cancel || modal.should_close() {
                    None
                } else {
                    Some(AddFlow::Probing { req, link, url, preselect, started })
                }
            }
            AddFlow::Error(msg) => {
                let mut close = false;
                let modal = egui::Modal::new(Id::new("probe-error")).frame(dialog_frame()).show(ctx, |ui| {
                    ui.set_width(480.0);
                    ui.label(RichText::new("Couldn't open this link").size(17.0).strong().color(DANGER));
                    ui.add_space(8.0);
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        ui.add(egui::Label::new(RichText::new(&msg).color(TEXT)).wrap().selectable(true));
                    });
                    ui.add_space(12.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(primary_button("OK")).clicked() {
                            close = true;
                        }
                        if ui.button("Copy Error").clicked() {
                            ui.ctx().copy_text(msg.clone());
                        }
                    });
                });
                if close || modal.should_close() {
                    None
                } else {
                    Some(AddFlow::Error(msg))
                }
            }
            AddFlow::Dialog(mut dlg) => match self.ui_dialog(ctx, &mut dlg) {
                DialogOutcome::Keep => Some(AddFlow::Dialog(dlg)),
                DialogOutcome::Close => None,
                DialogOutcome::Download => {
                    let mut remembered = dlg.opts.clone();
                    // Don't let an audio-only site flip the remembered default to audio.
                    if !dlg.link.site.has_video() {
                        remembered.media = self.settings.last_options.media;
                    }
                    self.settings.last_options = remembered;
                    self.settings_changed();
                    let selected = matches!(dlg.probe, ProbeResult::Playlist(_)).then_some(dlg.checked.as_slice());
                    self.start_from_probe(&dlg.probe, Some(dlg.link.site), selected, dlg.opts.clone());
                    None
                }
            },
        };
    }

    fn ui_choose(&mut self, ctx: &egui::Context, link: Link) -> Option<AddFlow> {
        let mut choice = None;
        let mut cancel = false;
        let modal = egui::Modal::new(Id::new("choose")).frame(dialog_frame()).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.label(RichText::new("This video is part of a playlist").size(17.0).strong());
            ui.add_space(10.0);
            if choice_button(ui, "⬇", "Download the Video", "Only the video from this link (v=…).").clicked() {
                choice = Some(0);
            }
            ui.add_space(6.0);
            if choice_button(ui, "☰", "Download the Playlist", "Save every video in the playlist.").clicked() {
                choice = Some(1);
            }
            ui.add_space(6.0);
            if choice_button(ui, "☑", "Choose from the Playlist", "Pick which videos of the playlist to download.").clicked()
            {
                choice = Some(2);
            }
            ui.add_space(12.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        let video_id = match &link.kind {
            LinkKind::VideoInPlaylist { video_id, .. } => video_id.clone(),
            _ => String::new(),
        };
        match choice {
            Some(0) => {
                let url = link.video_only_url();
                self.probe(link, url, true, Preselect::All, false);
                self.add.take()
            }
            Some(1) => {
                let url = link.playlist_url();
                self.probe(link, url, false, Preselect::All, false);
                self.add.take()
            }
            Some(_) => {
                let url = link.playlist_url();
                self.probe(link, url, false, Preselect::Only(video_id), false);
                self.add.take()
            }
            None if cancel || modal.should_close() => None,
            None => Some(AddFlow::Choose(link)),
        }
    }

    fn ui_dialog(&mut self, ctx: &egui::Context, dlg: &mut AddDialog) -> DialogOutcome {
        let mut outcome = DialogOutcome::Keep;
        let modal = egui::Modal::new(Id::new("add-dialog")).frame(dialog_frame()).show(ctx, |ui| {
            ui.set_width(680.0);
            match &dlg.probe {
                ProbeResult::Single(info) => single_header(ui, info, dlg.link.site),
                ProbeResult::Playlist(pl) => {
                    let pl = pl.clone();
                    playlist_picker(ui, &pl, &mut dlg.checked, &mut dlg.filter);
                }
            }
            ui.add_space(14.0);
            self.ui_options(ui, dlg);
            ui.add_space(16.0);

            ui.horizontal(|ui| {
                let estimate = match &dlg.probe {
                    ProbeResult::Single(info) => info.estimate_size(&dlg.opts).map(|s| format!("≈ {}", format_bytes(s))),
                    ProbeResult::Playlist(_) => None,
                };
                if let Some(e) = estimate {
                    ui.label(RichText::new(e).color(TEXT_DIM));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = match &dlg.probe {
                        ProbeResult::Single(_) => "Download".to_string(),
                        ProbeResult::Playlist(_) => {
                            let n = dlg.selected_count();
                            let noun = if dlg.opts.media == MediaKind::Audio { "Track" } else { "Video" };
                            format!("Download {n} {noun}{}", if n == 1 { "" } else { "s" })
                        }
                    };
                    let enabled = !matches!(dlg.probe, ProbeResult::Playlist(_)) || dlg.selected_count() > 0;
                    if ui.add_enabled(enabled, primary_button(&label).min_size(vec2(180.0, 38.0))).clicked() {
                        outcome = DialogOutcome::Download;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(vec2(110.0, 38.0))).clicked() {
                        outcome = DialogOutcome::Close;
                    }
                });
            });
        });
        if modal.should_close() && matches!(outcome, DialogOutcome::Keep) {
            outcome = DialogOutcome::Close;
        }
        outcome
    }

    fn ui_options(&mut self, ui: &mut egui::Ui, dlg: &mut AddDialog) {
        let site = dlg.link.site;
        let single = match &dlg.probe {
            ProbeResult::Single(i) => Some(i.clone()),
            ProbeResult::Playlist(_) => None,
        };
        let video_possible = site.has_video() && single.as_ref().is_none_or(|i| i.has_video());
        let o = &mut dlg.opts;

        ui.columns(3, |cols| {
            field_label(&mut cols[0], "Download");
            cols[0].add_enabled_ui(video_possible, |ui| {
                egui::ComboBox::from_id_salt("media")
                    .width(ui.available_width())
                    .selected_text(match o.media {
                        MediaKind::Video => "Video",
                        MediaKind::Audio => "Audio only",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut o.media, MediaKind::Video, "Video");
                        ui.selectable_value(&mut o.media, MediaKind::Audio, "Audio only");
                    });
            });

            field_label(&mut cols[1], "Format");
            let ui = &mut cols[1];
            match o.media {
                MediaKind::Video => {
                    egui::ComboBox::from_id_salt("container")
                        .width(ui.available_width())
                        .selected_text(o.container.label())
                        .show_ui(ui, |ui| {
                            for c in VideoContainer::ALL {
                                if c == VideoContainer::Webm && site != Site::YouTube {
                                    continue;
                                }
                                ui.selectable_value(&mut o.container, c, c.label());
                            }
                        });
                }
                MediaKind::Audio => {
                    egui::ComboBox::from_id_salt("audio-format")
                        .width(ui.available_width())
                        .selected_text(o.audio_format.label())
                        .show_ui(ui, |ui| {
                            for f in AudioFormat::ALL {
                                ui.selectable_value(&mut o.audio_format, f, f.label());
                            }
                        });
                }
            }

            let ui = &mut cols[2];
            match o.media {
                MediaKind::Video => {
                    field_label(ui, "Codec");
                    egui::ComboBox::from_id_salt("codec")
                        .width(ui.available_width())
                        .selected_text(o.codec.label())
                        .show_ui(ui, |ui| {
                            for c in VideoCodec::ALL {
                                ui.selectable_value(&mut o.codec, c, c.label());
                            }
                        });
                }
                MediaKind::Audio => {
                    field_label(ui, "Bitrate");
                    ui.add_enabled_ui(!o.audio_format.is_lossless(), |ui| {
                        egui::ComboBox::from_id_salt("bitrate")
                            .width(ui.available_width())
                            .selected_text(if o.audio_format.is_lossless() {
                                "Lossless".to_string()
                            } else {
                                o.audio_bitrate.label()
                            })
                            .show_ui(ui, |ui| {
                                for b in AUDIO_BITRATES {
                                    ui.selectable_value(&mut o.audio_bitrate, b, b.label());
                                }
                            });
                    });
                }
            }
        });

        if o.media == MediaKind::Video {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                field_label(ui, "Quality");
                if let Some(info) = &single {
                    if let Some(h) = info.effective_height(o.quality) {
                        ui.label(RichText::new(format!("· downloads {h}p")).small().color(TEXT_FAINT));
                    }
                }
            });
            widgets::quality_slider(ui, &mut o.quality, single.as_ref().map(|i| i.heights.as_slice()));

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                field_label(ui, "Subtitles");
                match single.as_ref().map(|i| i.subtitle_langs.clone()).filter(|l| !l.is_empty()) {
                    Some(langs) => {
                        let current = if o.subtitles.is_empty() { "None".to_string() } else { o.subtitles.clone() };
                        egui::ComboBox::from_id_salt("subs").selected_text(current).show_ui(ui, |ui| {
                            ui.selectable_value(&mut o.subtitles, String::new(), "None");
                            ui.selectable_value(&mut o.subtitles, "all".to_string(), "All languages");
                            for l in langs {
                                ui.selectable_value(&mut o.subtitles, l.clone(), l);
                            }
                        });
                    }
                    None => {
                        ui.add(
                            egui::TextEdit::singleline(&mut o.subtitles)
                                .hint_text("none (e.g. en,ja or all)")
                                .desired_width(200.0),
                        );
                    }
                }
            });
            if matches!(site, Site::Bilibili | Site::Niconico) {
                ui.add_space(4.0);
                let label = if site == Site::Bilibili {
                    "Save danmaku as subtitles (.danmaku.ass)"
                } else {
                    "Save comments as subtitles (.comments.ass)"
                };
                ui.checkbox(&mut o.comments, label)
                    .on_hover_text("Scrolling comments in a subtitle file next to the video; mpv and VLC load it automatically.");
            }
        } else if !o.audio_format.is_lossless() && o.audio_format != AudioFormat::M4a {
            ui.add_space(6.0);
            widgets::hint(ui, "The best available audio stream is downloaded and converted with FFmpeg.");
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            field_label(ui, "Save to");
            let mut path = o.output_dir.to_string_lossy().into_owned();
            let w = ui.available_width() - 100.0;
            if ui.add(egui::TextEdit::singleline(&mut path).desired_width(w)).changed() {
                o.output_dir = path.into();
            }
            if ui.button("Browse…").clicked() {
                self.pick_folder(PickTarget::DialogOutput, &o.output_dir.clone());
            }
        });
        if let ProbeResult::Playlist(pl) = &dlg.probe {
            if self.settings.playlist_subdirs {
                widgets::hint(ui, &format!("Files go into a subfolder named “{}”.", sanitize_filename(&pl.title)));
            }
        }
    }
}

fn single_header(ui: &mut egui::Ui, info: &MediaInfo, site: Site) {
    ui.horizontal(|ui| {
        let size = vec2(192.0, 108.0);
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        ui.painter().rect_filled(rect, CornerRadius::same(6), SURFACE_HI);
        if let Some(t) = &info.thumbnail {
            egui::Image::new(t.as_str()).corner_radius(CornerRadius::same(6)).paint_at(ui, rect);
        }
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.set_max_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(&info.title).size(16.0).strong()).wrap());
            let mut meta = vec![site.name().to_string()];
            if let Some(u) = &info.uploader {
                meta.push(u.clone());
            }
            if let Some(d) = info.duration {
                meta.push(format_duration(d));
            }
            if let Some(max) = info.heights.iter().max() {
                meta.push(format!("up to {max}p"));
            } else {
                meta.push("audio".into());
            }
            ui.label(RichText::new(meta.join(" · ")).color(TEXT_DIM));
            if info.is_live {
                ui.label(RichText::new("● Live stream: recording continues until it ends").small().color(DANGER));
            }
        });
    });
}

fn playlist_picker(ui: &mut egui::Ui, pl: &PlaylistInfo, checked: &mut [bool], filter: &mut String) {
    let n = pl.entries.len();
    let selected = checked.iter().filter(|c| **c).count();
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("Download Playlist: {n} item{} found", if n == 1 { "" } else { "s" })).size(17.0).strong());
    });
    ui.add(egui::Label::new(RichText::new(&pl.title).color(TEXT_DIM)).truncate());
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let mut all = selected == n && n > 0;
        let label = format!("{selected} of {n} selected");
        if ui.checkbox(&mut all, label).changed() {
            checked.iter_mut().for_each(|c| *c = all);
        }
        if ui.small_button("Invert").clicked() {
            checked.iter_mut().for_each(|c| *c = !*c);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(egui::TextEdit::singleline(filter).hint_text("🔍 Filter").desired_width(180.0));
        });
    });
    ui.add_space(4.0);
    let needle = filter.trim().to_lowercase();
    let visible: Vec<usize> =
        (0..n).filter(|i| needle.is_empty() || pl.entries[*i].title.to_lowercase().contains(&needle)).collect();
    let row_h = 46.0;
    egui::Frame::new().fill(FIELD).corner_radius(CornerRadius::same(8)).inner_margin(4.0).show(ui, |ui| {
        egui::ScrollArea::vertical().max_height(250.0).auto_shrink([false, true]).show_rows(ui, row_h, visible.len(), |ui, range| {
            for &i in &visible[range] {
                let e = &pl.entries[i];
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                if resp.clicked() {
                    checked[i] = !checked[i];
                }
                let painter = ui.painter();
                if resp.hovered() {
                    painter.rect_filled(rect, CornerRadius::same(6), SURFACE_HI);
                }
                let cb = Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), vec2(16.0, 16.0));
                painter.rect_filled(cb, CornerRadius::same(4), if checked[i] { ACCENT } else { SURFACE_HI });
                painter.rect_stroke(cb, CornerRadius::same(4), egui::Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
                if checked[i] {
                    painter.text(cb.center(), egui::Align2::CENTER_CENTER, "✔", FontId::proportional(11.0), egui::Color32::WHITE);
                }
                let thumb = Rect::from_min_size(pos2(rect.left() + 34.0, rect.center().y - 18.0), vec2(64.0, 36.0));
                painter.rect_filled(thumb, CornerRadius::same(4), SURFACE_HI);
                if let Some(t) = &e.thumbnail {
                    egui::Image::new(t.as_str()).corner_radius(CornerRadius::same(4)).show_loading_spinner(false).paint_at(ui, thumb);
                }
                let dur_w = 60.0;
                let text_x = thumb.right() + 10.0;
                let max_w = rect.right() - dur_w - text_x - 8.0;
                paint_line(
                    painter,
                    pos2(text_x, rect.center().y - 9.0),
                    &format!("{}. {}", e.index, e.title),
                    FontId::proportional(13.5),
                    if checked[i] { TEXT } else { TEXT_DIM },
                    max_w,
                );
                if let Some(d) = e.duration {
                    painter.text(
                        pos2(rect.right() - 10.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        format_duration(d),
                        FontId::proportional(12.5),
                        TEXT_DIM,
                    );
                }
            }
        });
    });
}
