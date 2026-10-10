//! Media converter dialog (convert local files to MP3/M4A/OGG/…).

use crate::app::{OdlApp, PickTarget};
use crate::theme::*;
use crate::widgets::{field_label, hint};
use eframe::egui::{self, vec2, Align, Id, Layout, RichText};
use odl_core::*;
use std::path::PathBuf;

pub struct ConvertView {
    pub files: Vec<PathBuf>,
    pub target: ConvertTarget,
    pub bitrate: AudioBitrate,
    pub same_folder: bool,
    pub output_dir: PathBuf,
}

impl Default for ConvertView {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            target: ConvertTarget::Audio(AudioFormat::Mp3),
            bitrate: AudioBitrate::Kbps(320),
            same_folder: true,
            output_dir: PathBuf::new(),
        }
    }
}

impl OdlApp {
    pub fn ui_convert(&mut self, ctx: &egui::Context) {
        let Some(mut view) = self.convert_view.take() else { return };
        let mut close = false;
        let mut start = false;
        let mut add_files = false;
        let mut browse = false;
        let modal = egui::Modal::new(Id::new("convert")).frame(dialog_frame()).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.label(RichText::new("Convert media files").size(17.0).strong());
            hint(ui, "Convert videos to audio, change audio formats, or make a video play everywhere (MP4/H.264).");
            ui.add_space(10.0);

            egui::Frame::new().fill(FIELD).corner_radius(8).inner_margin(8.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if view.files.is_empty() {
                    ui.label(RichText::new("No files yet. Add some, or drop them onto the window.").color(TEXT_DIM));
                }
                let mut remove = None;
                egui::ScrollArea::vertical().max_height(170.0).show(ui, |ui| {
                    for (i, f) in view.files.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ui.small_button("✖").clicked() {
                                remove = Some(i);
                            }
                            let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            ui.add(egui::Label::new(name).truncate()).on_hover_text(f.to_string_lossy());
                        });
                    }
                });
                if let Some(i) = remove {
                    view.files.remove(i);
                }
            });
            ui.add_space(6.0);
            add_files = ui.button("➕ Add files…").clicked();
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                field_label(ui, "Convert to");
                egui::ComboBox::from_id_salt("conv-target").selected_text(view.target.label()).width(160.0).show_ui(ui, |ui| {
                    for t in ConvertTarget::ALL {
                        ui.selectable_value(&mut view.target, t, t.label());
                    }
                });
                let lossy_audio = matches!(view.target, ConvertTarget::Audio(f) if !f.is_lossless());
                if lossy_audio {
                    field_label(ui, "Bitrate");
                    egui::ComboBox::from_id_salt("conv-bitrate").selected_text(view.bitrate.label()).width(110.0).show_ui(ui, |ui| {
                        for b in AUDIO_BITRATES {
                            ui.selectable_value(&mut view.bitrate, b, b.label());
                        }
                    });
                }
            });
            ui.add_space(6.0);
            ui.checkbox(&mut view.same_folder, "Save next to the original file");
            if !view.same_folder {
                ui.horizontal(|ui| {
                    let mut p = view.output_dir.to_string_lossy().into_owned();
                    if ui.add(egui::TextEdit::singleline(&mut p).desired_width(ui.available_width() - 100.0)).changed() {
                        view.output_dir = p.into();
                    }
                    browse = ui.button("Browse…").clicked();
                });
            }
            if self.tools.ffmpeg.is_none() {
                ui.add_space(6.0);
                ui.label(RichText::new("FFmpeg is required. Install it in Settings → Components.").color(DANGER));
            }
            ui.add_space(14.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let n = view.files.len();
                let label = format!("Convert {n} file{}", if n == 1 { "" } else { "s" });
                let ok = n > 0 && self.tools.ffmpeg.is_some() && (view.same_folder || !view.output_dir.as_os_str().is_empty());
                if ui.add_enabled(ok, primary_button(&label).min_size(vec2(160.0, 36.0))).clicked() {
                    start = true;
                }
                if ui.add(egui::Button::new("Cancel").min_size(vec2(100.0, 36.0))).clicked() {
                    close = true;
                }
            });
        });
        if add_files {
            self.pick_media_files();
        }
        if browse {
            let start_dir = if view.output_dir.as_os_str().is_empty() { self.settings.download_dir.clone() } else { view.output_dir.clone() };
            self.pick_folder(PickTarget::ConvertOutput, &start_dir);
        }
        if start {
            let out = (!view.same_folder).then(|| view.output_dir.clone());
            let bitrate = view.bitrate;
            for f in std::mem::take(&mut view.files) {
                self.start_conversion(f, view.target, bitrate, out.clone());
            }
            self.toast(crate::app::ToastKind::Success, "Conversion started");
            self.tab = crate::app::Tab::Converted;
            self.group_view = None;
            return;
        }
        if !(close || modal.should_close()) {
            self.convert_view = Some(view);
        }
    }
}
