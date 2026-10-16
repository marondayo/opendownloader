//! Settings window: general, downloads, connection, notifications, accounts, components, updates.

use crate::app::{BiliLogin, OdlApp, PickTarget, UpdateStatus};
use crate::theme::*;
use crate::widgets::{field_label, hint, section_title, setting_row, toggle};
use eframe::egui::{self, vec2, Align, Color32, CornerRadius, Id, Layout, Rect, RichText, Sense};
use odl_core::auth;
use odl_core::browser_cookies::{self, Readability};
use odl_core::settings::{AppUpdates, AuthMethod, Browser, ProxyKind};
use odl_core::tools::{Tool, ToolSource};
use odl_core::update::{self, Install};
use odl_core::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsPage {
    #[default]
    General,
    Downloads,
    Connection,
    Notifications,
    Accounts,
    Components,
    Updates,
}

impl SettingsPage {
    const ALL: [SettingsPage; 7] = [
        SettingsPage::General,
        SettingsPage::Downloads,
        SettingsPage::Connection,
        SettingsPage::Notifications,
        SettingsPage::Accounts,
        SettingsPage::Components,
        SettingsPage::Updates,
    ];
    fn label(self) -> &'static str {
        match self {
            SettingsPage::General => "General",
            SettingsPage::Downloads => "Downloads",
            SettingsPage::Connection => "Connection",
            SettingsPage::Notifications => "Notifications",
            SettingsPage::Accounts => "Accounts",
            SettingsPage::Components => "Components",
            SettingsPage::Updates => "Updates",
        }
    }
}

#[derive(Default)]
pub struct SettingsView {
    pub page: SettingsPage,
}

const SPEEDS: [(u32, &str); 8] = [
    (0, "Unlimited"),
    (256, "256 KB/s"),
    (512, "512 KB/s"),
    (1024, "1 MB/s"),
    (2048, "2 MB/s"),
    (5120, "5 MB/s"),
    (10240, "10 MB/s"),
    (20480, "20 MB/s"),
];

impl OdlApp {
    pub fn ui_settings(&mut self, ctx: &egui::Context) {
        let Some(mut page) = self.settings_view.as_ref().map(|v| v.page) else { return };
        let mut close = false;
        let mut changed = false;
        let screen = ctx.content_rect();
        let size = vec2((screen.width() - 80.0).clamp(640.0, 900.0), (screen.height() - 80.0).clamp(420.0, 640.0));
        let modal = egui::Modal::new(Id::new("settings"))
            .frame(egui::Frame::new().fill(SURFACE).stroke(egui::Stroke::new(1.0, BORDER)).corner_radius(CornerRadius::same(12)))
            .show(ctx, |ui| {
                ui.set_min_size(size);
                ui.set_max_size(size);
                const NAV_W: f32 = 200.0;
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.horizontal_top(|ui| {
                    // Navigation column: explicit top-down layout so the buttons stack.
                    ui.allocate_ui_with_layout(vec2(NAV_W, size.y), Layout::top_down(Align::Min), |ui| {
                        egui::Frame::new()
                            .fill(BG)
                            .corner_radius(CornerRadius { nw: 12, sw: 12, ne: 0, se: 0 })
                            .inner_margin(egui::Margin::symmetric(12, 16))
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing = vec2(8.0, 4.0);
                                ui.set_width(NAV_W - 24.0);
                                ui.set_min_height(size.y - 32.0);
                                ui.label(RichText::new("Settings").size(17.0).strong());
                                ui.add_space(12.0);
                                for p in SettingsPage::ALL {
                                    let active = p == page;
                                    let text = RichText::new(p.label()).size(14.5).color(if active { TEXT } else { TEXT_DIM });
                                    let button = egui::Button::selectable(active, text).min_size(vec2(NAV_W - 24.0, 34.0));
                                    if ui.add(button).clicked() {
                                        page = p;
                                    }
                                }
                            });
                    });
                    // Content column
                    ui.allocate_ui_with_layout(vec2(size.x - NAV_W, size.y), Layout::top_down(Align::Min), |ui| {
                        ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
                        ui.set_width(size.x - NAV_W);
                        ui.add_space(16.0);
                        ui.horizontal(|ui| {
                            ui.add_space(20.0);
                            ui.label(RichText::new(page.label()).size(18.0).strong());
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.add_space(14.0);
                                if ui.add(egui::Button::new(RichText::new("✖").size(15.0)).frame(false)).clicked() {
                                    close = true;
                                }
                            });
                        });
                        ui.add_space(6.0);
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                            egui::Frame::new().inner_margin(egui::Margin { left: 20, right: 24, top: 0, bottom: 16 }).show(ui, |ui| {
                                changed |= match page {
                                    SettingsPage::General => self.page_general(ui),
                                    SettingsPage::Downloads => self.page_downloads(ui),
                                    SettingsPage::Connection => self.page_connection(ui),
                                    SettingsPage::Notifications => self.page_notifications(ui),
                                    SettingsPage::Accounts => self.page_accounts(ui),
                                    SettingsPage::Components => self.page_components(ui),
                                    SettingsPage::Updates => self.page_updates(ui),
                                };
                            });
                        });
                    });
                });
            });
        if changed {
            self.settings_changed();
        }
        if close || modal.should_close() {
            self.settings_view = None;
        } else if let Some(v) = &mut self.settings_view {
            v.page = page;
        }
    }

    fn page_general(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let mut browse_default = false;
        let mut browse_smart = false;
        let s = &mut self.settings;
        section_title(ui, "Default folder");
        ui.horizontal(|ui| {
            let mut path = s.download_dir.to_string_lossy().into_owned();
            if ui.add(egui::TextEdit::singleline(&mut path).desired_width(ui.available_width() - 100.0)).changed() {
                s.download_dir = path.into();
                s.last_options.output_dir = s.download_dir.clone();
                changed = true;
            }
            browse_default = ui.button("Browse…").clicked();
        });
        ui.add_space(10.0);
        section_title(ui, "Smart Mode");
        changed |= setting_row(ui, "Enable Smart Mode", Some("Pasting a link starts downloading right away with the preset below."), |ui| {
            toggle(ui, &mut s.smart_mode).changed()
        });
        let p = &mut s.smart_preset;
        ui.horizontal(|ui| {
            field_label(ui, "Preset");
            egui::ComboBox::from_id_salt("smart-media")
                .selected_text(if p.media == MediaKind::Video { "Video" } else { "Audio" })
                .width(110.0)
                .show_ui(ui, |ui| {
                    changed |= ui.selectable_value(&mut p.media, MediaKind::Video, "Video").changed();
                    changed |= ui.selectable_value(&mut p.media, MediaKind::Audio, "Audio").changed();
                });
            if p.media == MediaKind::Video {
                egui::ComboBox::from_id_salt("smart-container").selected_text(p.container.label()).width(90.0).show_ui(ui, |ui| {
                    for c in VideoContainer::ALL {
                        changed |= ui.selectable_value(&mut p.container, c, c.label()).changed();
                    }
                });
                egui::ComboBox::from_id_salt("smart-quality").selected_text(p.quality.label()).width(90.0).show_ui(ui, |ui| {
                    for q in QUALITY_STEPS {
                        changed |= ui.selectable_value(&mut p.quality, q, q.label()).changed();
                    }
                });
            } else {
                egui::ComboBox::from_id_salt("smart-audio").selected_text(p.audio_format.label()).width(90.0).show_ui(ui, |ui| {
                    for f in AudioFormat::ALL {
                        changed |= ui.selectable_value(&mut p.audio_format, f, f.label()).changed();
                    }
                });
                egui::ComboBox::from_id_salt("smart-bitrate").selected_text(p.audio_bitrate.label()).width(100.0).show_ui(ui, |ui| {
                    for b in AUDIO_BITRATES {
                        changed |= ui.selectable_value(&mut p.audio_bitrate, b, b.label()).changed();
                    }
                });
            }
        });
        ui.horizontal(|ui| {
            field_label(ui, "Smart Mode folder");
            let mut path = p.output_dir.to_string_lossy().into_owned();
            if ui.add(egui::TextEdit::singleline(&mut path).desired_width(ui.available_width() - 100.0)).changed() {
                p.output_dir = path.into();
                changed = true;
            }
            browse_smart = ui.button("Browse…").clicked();
        });
        ui.add_space(10.0);
        section_title(ui, "Clipboard");
        changed |= setting_row(
            ui,
            "Watch the clipboard for links",
            Some("When you copy a YouTube, Niconico, Bilibili or SoundCloud link anywhere, offer to download it."),
            |ui| toggle(ui, &mut s.monitor_clipboard).changed(),
        );
        changed |= setting_row(
            ui,
            "In Smart Mode, download copied links immediately",
            Some("No confirmation: copying a link starts the download with the Smart Mode preset."),
            |ui| toggle(ui, &mut s.clipboard_smart_auto).changed(),
        );

        ui.add_space(10.0);
        section_title(ui, "Behavior");
        changed |= setting_row(ui, "Ask to confirm quitting while downloads are running", None, |ui| {
            toggle(ui, &mut s.confirm_exit_with_active).changed()
        });
        changed |= setting_row(ui, "Remove finished downloads from the list", None, |ui| toggle(ui, &mut s.remove_completed).changed());
        if browse_default {
            self.pick_folder(PickTarget::DefaultDir, &self.settings.download_dir);
        }
        if browse_smart {
            self.pick_folder(PickTarget::SmartDir, &self.settings.smart_preset.output_dir);
        }
        changed
    }

    fn page_downloads(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let s = &mut self.settings;
        section_title(ui, "Simultaneous downloads");
        ui.horizontal(|ui| {
            let mut n = s.max_concurrent as u32;
            let r = ui.add(egui::Slider::new(&mut n, 1..=8).show_value(true));
            if r.changed() {
                s.max_concurrent = n as usize;
                changed = true;
            }
            let label = match n {
                1..=2 => "Safe",
                3..=4 => "Stable",
                5..=6 => "Fast",
                _ => "Risky",
            };
            ui.label(RichText::new(label).color(if n > 6 { WARN } else { TEXT_DIM }));
        });
        hint(ui, "Many parallel downloads can make YouTube temporarily block your IP address.");
        ui.add_space(4.0);
        changed |= setting_row(ui, "Fragment threads", Some("Parallel connections per video for HLS/DASH streams (Niconico, Bilibili, …)."), |ui| {
            ui.add(egui::DragValue::new(&mut s.fragment_threads).range(1..=16)).changed()
        });

        section_title(ui, "Playlists");
        changed |= setting_row(ui, "Create subfolders for playlists and channels", None, |ui| toggle(ui, &mut s.playlist_subdirs).changed());
        changed |= setting_row(ui, "Number files in playlists (01. Title)", None, |ui| toggle(ui, &mut s.numerate_playlist_files).changed());
        changed |= setting_row(ui, "Skip duplicate entries in playlists", None, |ui| toggle(ui, &mut s.skip_duplicates).changed());
        changed |= setting_row(ui, "Generate .m3u files for downloaded playlists", None, |ui| toggle(ui, &mut s.generate_m3u).changed());
        changed |= setting_row(
            ui,
            "Never download the same video twice",
            Some("Keeps a download archive and skips anything already downloaded."),
            |ui| toggle(ui, &mut s.use_download_archive).changed(),
        );

        section_title(ui, "Subscriptions");
        let label = |h: u32| match h {
            0 => "Never (only when I click Check)".to_string(),
            1 => "Every hour".to_string(),
            h => format!("Every {h} hours"),
        };
        setting_row(ui, "Check subscriptions for new videos", None, |ui| {
            egui::ComboBox::from_id_salt("sub-interval").selected_text(label(s.subscription_check_hours)).width(220.0).show_ui(ui, |ui| {
                for h in [0, 1, 3, 6, 12, 24] {
                    changed |= ui.selectable_value(&mut s.subscription_check_hours, h, label(h)).changed();
                }
            });
        });

        section_title(ui, "Files");
        changed |= setting_row(ui, "Embed thumbnail / cover art", None, |ui| toggle(ui, &mut s.embed_thumbnail).changed());
        changed |= setting_row(ui, "Embed metadata (title, artist, date…)", None, |ui| toggle(ui, &mut s.embed_metadata).changed());
        changed |= setting_row(ui, "Embed chapters", None, |ui| toggle(ui, &mut s.embed_chapters).changed());
        changed |= setting_row(ui, "Embed subtitles in the video file when possible", None, |ui| toggle(ui, &mut s.embed_subtitles).changed());
        changed |= setting_row(
            ui,
            "Keep the original danmaku / comment file",
            Some("Also saves Bilibili danmaku as .xml and Niconico comments as .json, next to the .ass."),
            |ui| toggle(ui, &mut s.keep_raw_comments).changed(),
        );
        changed |= setting_row(
            ui,
            "Remove sponsor segments (YouTube, SponsorBlock)",
            Some("Cuts community-marked sponsor segments out of the video."),
            |ui| toggle(ui, &mut s.sponsorblock).changed(),
        );

        section_title(ui, "Advanced");
        field_label(ui, "File name template (yt-dlp output template)");
        changed |= ui.add(egui::TextEdit::singleline(&mut s.filename_template).desired_width(f32::INFINITY)).changed();
        hint(ui, "Examples: %(title)s.%(ext)s · %(uploader)s - %(title)s.%(ext)s · %(upload_date)s %(title)s.%(ext)s");
        ui.add_space(6.0);
        field_label(ui, "Extra yt-dlp arguments");
        changed |= ui
            .add(egui::TextEdit::singleline(&mut s.extra_ytdlp_args).hint_text("e.g. --limit-rate 5M").desired_width(f32::INFINITY))
            .changed();

        section_title(ui, "Disk space");
        let label = |mib: u64| match mib {
            0 => "No limit (not recommended)".to_string(),
            m if m < 1024 => format!("{m} MB"),
            m => format!("{} GB", m / 1024),
        };
        setting_row(
            ui,
            "Always keep this much free",
            Some("Downloads that don't fit are refused, and running ones stop (resumable) before the disk fills up."),
            |ui| {
                egui::ComboBox::from_id_salt("keep-free").selected_text(label(s.keep_free_mib)).width(200.0).show_ui(ui, |ui| {
                    for m in [0, 512, 1024, 2048, 5120, 10240, 20480, 51200] {
                        changed |= ui.selectable_value(&mut s.keep_free_mib, m, label(m)).changed();
                    }
                });
            },
        );
        let (mut scan, mut delete) = (false, false);
        setting_row(
            ui,
            "Leftover partial downloads",
            Some("Unfinished downloads removed from the list (or from older sessions) can leave partial files behind."),
            |ui| {
                match &self.leftovers {
                    Some(found) if !found.is_empty() => {
                        delete = ui.add(egui::Button::new(RichText::new("Delete").color(Color32::WHITE)).fill(DANGER)).clicked();
                        let bytes: u64 = found.iter().map(|l| l.bytes).sum();
                        ui.label(RichText::new(format!("{} in {} folder{}", format_bytes(bytes), found.len(), if found.len() == 1 { "" } else { "s" })).color(WARN));
                    }
                    Some(_) => {
                        scan = ui.button("Scan again").clicked();
                        ui.label(RichText::new("✔ Nothing left over").color(ACCENT_HOVER));
                    }
                    None => scan = ui.button("Scan").clicked(),
                }
            },
        );
        if scan {
            self.scan_leftovers();
        }
        if delete {
            self.delete_leftovers();
        }
        changed
    }

    fn page_connection(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let s = &mut self.settings;
        section_title(ui, "Speed limit");
        let current = SPEEDS.iter().find(|(k, _)| *k == s.rate_limit_kib).map(|(_, l)| l.to_string()).unwrap_or_else(|| format!("{} KB/s", s.rate_limit_kib));
        egui::ComboBox::from_id_salt("speed").selected_text(current).show_ui(ui, |ui| {
            for (k, label) in SPEEDS {
                changed |= ui.selectable_value(&mut s.rate_limit_kib, k, label).changed();
            }
        });
        hint(ui, "Limits each download. Useful on slow or shared connections.");

        ui.add_space(10.0);
        section_title(ui, "Bilibili server");
        let label = |v: &str| match v {
            "" => "As provided by Bilibili".to_string(),
            odl_core::bilicdn::AUTO => "Automatic (fastest)".to_string(),
            host => host.to_string(),
        };
        egui::ComboBox::from_id_salt("bili-cdn").selected_text(label(&s.bilibili_cdn)).width(300.0).show_ui(ui, |ui| {
            changed |= ui.selectable_value(&mut s.bilibili_cdn, odl_core::bilicdn::AUTO.to_string(), label(odl_core::bilicdn::AUTO)).changed();
            changed |= ui.selectable_value(&mut s.bilibili_cdn, String::new(), label("")).changed();
            for host in odl_core::bilicdn::MIRRORS {
                changed |= ui.selectable_value(&mut s.bilibili_cdn, host.to_string(), *host).changed();
            }
        });
        hint(ui, "Bilibili's default overseas server often crawls on parts of a video it hasn't cached. \
                  Automatic tests its official mirrors before each download and uses the fastest.");

        ui.add_space(10.0);
        section_title(ui, "Proxy");
        let p = &mut s.proxy;
        changed |= setting_row(ui, "Use a proxy", Some("Also helps with region-locked videos."), |ui| toggle(ui, &mut p.enabled).changed());
        ui.add_enabled_ui(p.enabled, |ui| {
            egui::Grid::new("proxy").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label("Type");
                egui::ComboBox::from_id_salt("proxy-kind").selected_text(p.kind.scheme().to_uppercase()).show_ui(ui, |ui| {
                    for k in ProxyKind::ALL {
                        changed |= ui.selectable_value(&mut p.kind, k, k.scheme().to_uppercase()).changed();
                    }
                });
                ui.end_row();
                ui.label("Host");
                changed |= ui.add(egui::TextEdit::singleline(&mut p.host).hint_text("127.0.0.1")).changed();
                ui.end_row();
                ui.label("Port");
                changed |= ui.add(egui::DragValue::new(&mut p.port).range(0..=65535)).changed();
                ui.end_row();
                ui.label("Login");
                changed |= ui.add(egui::TextEdit::singleline(&mut p.username).hint_text("optional")).changed();
                ui.end_row();
                ui.label("Password");
                changed |= ui.add(egui::TextEdit::singleline(&mut p.password).password(true).hint_text("optional")).changed();
                ui.end_row();
            });
        });
        if let Some(url) = p.url() {
            let shown = if p.password.is_empty() { url } else { url.replace(&p.password, "•••") };
            hint(ui, &format!("Using {shown}"));
        }
        changed
    }

    fn page_notifications(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let s = &mut self.settings;
        changed |= setting_row(ui, "Show a notification when a download finishes", None, |ui| toggle(ui, &mut s.notify_download_finished).changed());
        changed |= setting_row(ui, "Show a notification when a conversion finishes", None, |ui| toggle(ui, &mut s.notify_conversion_finished).changed());
        if ui.button("Send a test notification").clicked() {
            crate::platform::notify("OpenDownloader", "Notifications are working.");
        }
        changed
    }

    // -----------------------------------------------------------------------

    fn page_accounts(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        hint(
            ui,
            "Signing in unlocks private, members-only, age-restricted and premium-quality content. \
             Your credentials stay on this computer and are only handed to yt-dlp.",
        );
        ui.add_space(8.0);
        for site in Site::ALL {
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                changed |= self.account_card(ui, site);
            });
            ui.add_space(10.0);
        }
        changed
    }

    fn account_status(&self, site: Site) -> (String, Color32) {
        let acct = self.settings.account(site);
        match acct.method {
            AuthMethod::None => ("Not signed in".into(), TEXT_DIM),
            AuthMethod::Browser => (format!("Using cookies from {}", acct.browser.label()), ACCENT_HOVER),
            AuthMethod::CookiesFile => match &acct.cookies_file {
                Some(p) if p.exists() => ("Using cookies.txt".into(), ACCENT_HOVER),
                _ => ("cookies.txt missing".into(), DANGER),
            },
            AuthMethod::Password if auth::has_credentials(site) => {
                let who = if site == Site::SoundCloud { "OAuth token saved".to_string() } else { format!("Signed in as {}", acct.username) };
                (who, ACCENT_HOVER)
            }
            AuthMethod::Password => ("No credentials saved".into(), WARN),
            AuthMethod::QrCode if auth::managed_cookie_file(site).exists() => match &acct.display_name {
                Some(n) => (format!("Signed in as {n}"), ACCENT_HOVER),
                None => ("Signed in".into(), ACCENT_HOVER),
            },
            AuthMethod::QrCode => ("Not signed in".into(), TEXT_DIM),
        }
    }

    fn account_card(&mut self, ui: &mut egui::Ui, site: Site) -> bool {
        let mut changed = false;
        let (status, color) = self.account_status(site);
        ui.horizontal(|ui| {
            ui.label(RichText::new(site.name()).size(15.5).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.settings.account(site).method != AuthMethod::None && ui.small_button("Sign out").clicked() {
                    self.sign_out(site);
                }
                ui.label(RichText::new(status).color(color));
            });
        });
        ui.add_space(6.0);

        let mut method = self.settings.account(site).method;
        ui.horizontal(|ui| {
            field_label(ui, "Method");
            egui::ComboBox::from_id_salt(("auth-method", site)).selected_text(method.label()).width(220.0).show_ui(ui, |ui| {
                for m in AuthMethod::for_site(site) {
                    ui.selectable_value(&mut method, *m, m.label());
                }
            });
        });
        if method != self.settings.account(site).method {
            self.settings.account_mut(site).method = method;
            changed = true;
        }
        ui.add_space(4.0);

        match method {
            AuthMethod::None => hint(ui, match site {
                Site::YouTube => "Needed for private, members-only and age-restricted videos, or when YouTube asks you to “confirm you're not a bot”.",
                Site::Niconico => "Premium accounts get the highest quality; some videos require any account.",
                Site::Bilibili => "Without signing in Bilibili limits quality to 480p, and some networks are blocked entirely.",
                Site::SoundCloud => "Signing in with a Go+ account allows high-quality (256 kbps AAC) streams.",
            }),
            AuthMethod::Browser => {
                let acct = self.settings.account_mut(site);
                ui.horizontal(|ui| {
                    field_label(ui, "Browser");
                    egui::ComboBox::from_id_salt(("browser", site)).selected_text(acct.browser.label()).width(170.0).show_ui(ui, |ui| {
                        for b in Browser::ALL {
                            changed |= ui.selectable_value(&mut acct.browser, b, b.label()).changed();
                        }
                    });
                    field_label(ui, "Profile");
                    changed |= ui
                        .add(egui::TextEdit::singleline(&mut acct.browser_profile).hint_text("default").desired_width(150.0))
                        .changed();
                });
                let (browser, profile) = (acct.browser, acct.browser_profile.clone());
                if cfg!(windows) && browser != Browser::Firefox {
                    if changed {
                        self.browser_checks.remove(&site);
                    }
                    let check = *self.browser_checks.entry(site).or_insert_with(|| browser_cookies::check(browser, &profile));
                    let name = browser.label();
                    let (text, color) = match check {
                        Readability::Readable => (format!("✔ {name}'s cookies can be read"), ACCENT_HOVER),
                        Readability::Locked => (format!("✖ {name} is open and locks its cookies. Downloads continue without signing in."), DANGER),
                        Readability::AppBound => (
                            format!("✖ {name} encrypts its cookies in a way yt-dlp can't read on Windows. Use Firefox or a cookies.txt file."),
                            DANGER,
                        ),
                        Readability::NotFound => (format!("⚠ No {name} profile found"), WARN),
                    };
                    let mut recheck = false;
                    ui.horizontal(|ui| {
                        recheck = ui.button("Check").clicked();
                        ui.add(egui::Label::new(RichText::new(text).color(color)).wrap());
                    });
                    if recheck {
                        self.browser_checks.insert(site, browser_cookies::check(browser, &profile));
                    }
                    if check != Readability::AppBound {
                        hint(ui, &format!(
                            "{name} keeps its cookies locked while it's open, so OpenDownloader can only sign in with them \
                             while {name} is closed. To stay signed in with {name} open, use a cookies.txt file exported \
                             from a private window instead."
                        ));
                    }
                    if check != Readability::Readable && ui.button("Use a cookies.txt file instead").clicked() {
                        self.settings.account_mut(site).method = AuthMethod::CookiesFile;
                        changed = true;
                    }
                } else {
                    hint(ui, &format!(
                        "Sign in to {} in that browser first. On Linux, Chromium-based browsers may ask for keyring access; \
                         Firefox works best. Close the browser if its cookie database is locked.",
                        site.name()
                    ));
                }
            }
            AuthMethod::CookiesFile => {
                let acct = self.settings.account(site);
                ui.horizontal(|ui| {
                    let label = acct.cookies_file.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "No file selected".into());
                    ui.add(egui::Label::new(RichText::new(label).color(TEXT_DIM)).truncate());
                });
                ui.horizontal(|ui| {
                    if ui.button("Choose cookies.txt…").clicked() {
                        self.pick_cookie_file(site);
                    }
                    if let Some(p) = acct.cookies_file.clone() {
                        if ui.button("Check").clicked() {
                            self.cookie_checks.insert(site, auth::check_cookie_file(&p, site).map_err(|e| e.to_string()));
                        }
                    }
                });
                match self.cookie_checks.get(&site) {
                    Some(Ok(c)) if c.has_login => {
                        ui.label(RichText::new(format!("✔ Login session found ({} cookies for {})", c.site_cookies, site.name())).color(ACCENT_HOVER));
                    }
                    Some(Ok(c)) if c.expired_login => {
                        ui.label(RichText::new("✖ The login session in this file has expired. Export it again.").color(DANGER));
                    }
                    Some(Ok(c)) => {
                        ui.label(RichText::new(format!("⚠ No {} login cookie found ({} cookies in file)", site.name(), c.total)).color(WARN));
                    }
                    Some(Err(e)) => {
                        ui.label(RichText::new(e).color(DANGER));
                    }
                    None => {}
                }
                hint(ui, if site == Site::YouTube {
                    "YouTube rotates the cookies of an open session, so export them from a private window: sign in to \
                     YouTube in a new private window, open youtube.com/robots.txt in that tab, export the youtube.com \
                     cookies with a cookies.txt extension (e.g. “Get cookies.txt LOCALLY”), then close the private \
                     window without signing out."
                } else {
                    "Export a Netscape-format cookies.txt while signed in, e.g. with a “Get cookies.txt LOCALLY” browser extension."
                });
            }
            AuthMethod::Password => changed |= self.password_form(ui, site),
            AuthMethod::QrCode => self.bili_qr_ui(ui),
        }
        changed
    }

    fn password_form(&mut self, ui: &mut egui::Ui, site: Site) -> bool {
        let mut changed = false;
        let saved = auth::has_credentials(site);
        let (login, secret) = self.cred_inputs.entry(site).or_default();
        egui::Grid::new(("cred", site)).num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            if site == Site::SoundCloud {
                ui.label("OAuth token");
                ui.add(egui::TextEdit::singleline(secret).password(true).hint_text(if saved { "saved (enter a new one to replace)" } else { "2-123456-…" }).desired_width(280.0));
                ui.end_row();
            } else {
                ui.label("Email / username");
                ui.add(egui::TextEdit::singleline(login).desired_width(280.0));
                ui.end_row();
                ui.label("Password");
                ui.add(egui::TextEdit::singleline(secret).password(true).hint_text(if saved { "saved (enter to replace)" } else { "" }).desired_width(280.0));
                ui.end_row();
            }
        });
        let (login, secret) = self.cred_inputs.get(&site).cloned().unwrap_or_default();
        let login = if site == Site::SoundCloud { "oauth".to_string() } else { login };
        ui.horizontal(|ui| {
            let ready = !secret.is_empty() && !login.trim().is_empty();
            if ui.add_enabled(ready, primary_button("Save")).clicked() {
                match auth::set_credentials(site, login.trim(), &secret) {
                    Ok(()) => {
                        let acct = self.settings.account_mut(site);
                        acct.username = login.trim().to_string();
                        acct.method = AuthMethod::Password;
                        changed = true;
                        self.cred_inputs.remove(&site);
                        self.toast(crate::app::ToastKind::Success, format!("{} credentials saved", site.name()));
                    }
                    Err(e) => self.toast(crate::app::ToastKind::Error, format!("Saving failed: {e:#}")),
                }
            }
            if saved && ui.button("Forget").clicked() {
                let _ = auth::remove_credentials(site);
                self.settings.account_mut(site).username.clear();
                changed = true;
            }
        });
        if site == Site::SoundCloud {
            hint(ui, "Sign in on soundcloud.com, open the browser's developer tools → Storage/Application → Cookies and copy the value of “oauth_token”.");
        } else {
            hint(ui, "Stored in a private netrc file read by yt-dlp. Accounts with two-step verification should use browser cookies instead.");
        }
        changed
    }

    fn bili_qr_ui(&mut self, ui: &mut egui::Ui) {
        let signed_in = auth::managed_cookie_file(Site::Bilibili).exists();
        match &self.bili {
            BiliLogin::Idle if signed_in => {
                ui.horizontal(|ui| {
                    if ui.button("Check session").clicked() {
                        self.refresh_bili_account();
                    }
                    if ui.button("Sign in again").clicked() {
                        self.start_bili_login();
                    }
                });
            }
            BiliLogin::Idle => {
                if ui.add(primary_button("Show QR code")).clicked() {
                    self.start_bili_login();
                }
                hint(ui, "Scan the code with the Bilibili mobile app (我的 → 扫一扫) to sign in.");
            }
            BiliLogin::Loading => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Requesting a QR code…");
                });
            }
            BiliLogin::Showing { modules, width, scanned } => {
                let (modules, width, scanned) = (modules.clone(), *width, *scanned);
                ui.horizontal(|ui| {
                    paint_qr(ui, &modules, width, 196.0);
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        if scanned {
                            ui.label(RichText::new("✔ Scanned").color(ACCENT_HOVER).strong());
                            ui.label("Confirm the login on your phone.");
                        } else {
                            ui.label(RichText::new("Scan with the Bilibili app").strong());
                            ui.label(RichText::new("Open the app → 我的 (Me) → scan icon.").color(TEXT_DIM));
                        }
                        ui.add_space(8.0);
                        if ui.button("Cancel").clicked() {
                            self.cancel_bili_login();
                        }
                    });
                });
            }
            BiliLogin::Expired => {
                ui.label(RichText::new("The QR code expired.").color(WARN));
                if ui.button("Get a new code").clicked() {
                    self.start_bili_login();
                }
            }
            BiliLogin::Error(e) => {
                ui.label(RichText::new(e).color(DANGER));
                if ui.button("Try again").clicked() {
                    self.start_bili_login();
                }
            }
        }
    }

    // -----------------------------------------------------------------------

    fn page_components(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        hint(ui, "OpenDownloader drives these open-source programs. It can install and update private copies for you.");
        ui.add_space(8.0);
        for tool in Tool::ALL {
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                changed |= self.component_card(ui, tool);
            });
            ui.add_space(10.0);
        }
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            self.pot_card(ui);
        });
        ui.add_space(10.0);
        changed |= setting_row(
            ui,
            "Keep yt-dlp up to date automatically",
            Some("Checks once a day. Sites change often, and new yt-dlp releases keep downloads working."),
            |ui| toggle(ui, &mut self.settings.auto_update_ytdlp).changed(),
        );
        changed
    }

    fn component_card(&mut self, ui: &mut egui::Ui, tool: Tool) -> bool {
        let mut changed = false;
        let resolved = self.tools.get(tool).cloned();
        let state = self.tool_state.get(&tool);
        let version = state.and_then(|s| s.version.clone());
        let busy = state.and_then(|s| s.busy.clone());
        let error = state.and_then(|s| s.error.clone());
        ui.horizontal(|ui| {
            ui.label(RichText::new(tool.label()).size(15.5).strong());
            match (&resolved, &version) {
                (Some(_), Some(v)) => ui.label(RichText::new(format!("✔ {v}")).color(ACCENT_HOVER)),
                (Some(_), None) => ui.label(RichText::new("checking…").color(TEXT_DIM)),
                (None, _) => ui.label(RichText::new(if tool == Tool::Deno { "not installed (optional)" } else { "✖ not installed" }).color(if tool == Tool::Deno { WARN } else { DANGER })),
            };
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(b) = &busy {
                    ui.label(RichText::new(b).small().color(TEXT_DIM));
                    ui.spinner();
                    return;
                }
                match &resolved {
                    None => {
                        if ui.add(primary_button("Install")).clicked() {
                            self.install_tool(tool);
                        }
                    }
                    Some(r) => {
                        if tool == Tool::YtDlp {
                            let newer = match (&self.latest_ytdlp, &version) {
                                (Some(l), Some(v)) if odl_core::tools::is_newer_version(v, l) => Some(l.clone()),
                                _ => None,
                            };
                            let label = newer.map(|l| format!("Update to {l}")).unwrap_or_else(|| "Update".into());
                            if ui.button(label).clicked() {
                                self.update_ytdlp();
                            }
                            if self.latest_ytdlp.is_none() && ui.small_button("Check").clicked() {
                                self.check_ytdlp_latest();
                            }
                        } else if r.source == ToolSource::Managed && ui.button("Reinstall").clicked() {
                            self.install_tool(tool);
                        }
                        if r.source == ToolSource::System && tool != Tool::YtDlp && ui.button("Install private copy").clicked() {
                            self.install_tool(tool);
                        }
                    }
                }
            });
        });
        hint(ui, tool.purpose());
        if let Some(r) = &resolved {
            let source = match r.source {
                ToolSource::Custom => "custom path",
                ToolSource::Managed => "managed by OpenDownloader",
                ToolSource::System => "system",
            };
            ui.label(RichText::new(format!("{} ({source})", r.path.display())).small().color(TEXT_FAINT));
        } else if tool == Tool::Ffmpeg && cfg!(target_os = "linux") {
            hint(ui, "You can also install it with your package manager, e.g. sudo apt install ffmpeg / sudo dnf install ffmpeg / sudo pacman -S ffmpeg.");
        }
        if let Some(e) = &error {
            ui.add(egui::Label::new(RichText::new(e).small().color(DANGER)).wrap());
        }
        ui.horizontal(|ui| {
            if ui.small_button("Use custom path…").clicked() {
                self.pick_tool_path(tool);
            }
            let has_custom = match tool {
                Tool::YtDlp => self.settings.ytdlp_path.is_some(),
                Tool::Ffmpeg => self.settings.ffmpeg_path.is_some(),
                Tool::Deno => self.settings.deno_path.is_some(),
            };
            if has_custom && ui.small_button("Reset to automatic").clicked() {
                match tool {
                    Tool::YtDlp => self.settings.ytdlp_path = None,
                    Tool::Ffmpeg => self.settings.ffmpeg_path = None,
                    Tool::Deno => self.settings.deno_path = None,
                }
                changed = true;
                self.settings_changed();
                let settings = self.settings.clone();
                self.tools = odl_core::tools::resolve(&settings);
                self.refresh_tool_versions();
            }
        });
        changed
    }

    fn pot_card(&mut self, ui: &mut egui::Ui) {
        let st = &self.pot_state;
        let (version, busy, error) = (st.version.clone(), st.busy.clone(), st.error.clone());
        let has_deno = self.tools.deno.is_some();
        ui.horizontal(|ui| {
            ui.label(RichText::new("YouTube PO-token provider").size(15.5).strong());
            match &version {
                Some(v) => ui.label(RichText::new(format!("✔ {v}")).color(ACCENT_HOVER)),
                None => ui.label(RichText::new("not installed (optional)").color(TEXT_DIM)),
            };
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(b) = &busy {
                    ui.label(RichText::new(b).small().color(TEXT_DIM));
                    ui.spinner();
                    return;
                }
                match &version {
                    None => {
                        if ui.add_enabled(has_deno, primary_button("Install")).clicked() {
                            self.install_pot();
                        }
                    }
                    Some(_) => {
                        if ui.button("Remove").clicked() {
                            self.remove_pot();
                        }
                        if ui.add_enabled(has_deno, egui::Button::new("Update")).clicked() {
                            self.install_pot();
                        }
                    }
                }
            });
        });
        hint(
            ui,
            "Lets YouTube's mobile site serve full quality when YouTube asks to \"confirm you're not a bot\" \
             and the video can't be embedded. Each YouTube video takes a few seconds longer to start. \
             Uses Deno; about 250 MB.",
        );
        if !has_deno && version.is_none() {
            hint(ui, "Needs Deno (above).");
        }
        if let Some(e) = &error {
            ui.add(egui::Label::new(RichText::new(e).small().color(DANGER)).wrap());
        }
    }
}

impl OdlApp {
    fn page_updates(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let (mut check, mut install, mut skip, mut restart) = (false, false, false, false);
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let u = &self.app_update;
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("OpenDownloader {}", update::CURRENT_VERSION)).size(15.5).strong());
                match &u.status {
                    UpdateStatus::UpToDate => {
                        ui.label(RichText::new("✔ Up to date").color(ACCENT_HOVER));
                    }
                    UpdateStatus::Available(r) => {
                        ui.label(RichText::new(format!("Version {} is available", r.version)).color(WARN));
                    }
                    UpdateStatus::Ready(r, _) => {
                        ui.label(RichText::new(format!("✔ Version {} is ready", r.version)).color(ACCENT_HOVER));
                    }
                    _ => {}
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| match &u.status {
                    UpdateStatus::Checking => {
                        ui.label(RichText::new("Checking…").small().color(TEXT_DIM));
                        ui.spinner();
                    }
                    UpdateStatus::Downloading(_, progress) => {
                        ui.label(RichText::new(progress).small().color(TEXT_DIM));
                        ui.spinner();
                    }
                    UpdateStatus::Available(r) => {
                        let label = if r.installable(&u.install) { "Update now" } else { "Download" };
                        install = ui.add(primary_button(label)).clicked();
                        if self.settings.skipped_app_version != r.version {
                            skip = ui.button("Skip this version").clicked();
                        }
                    }
                    UpdateStatus::Ready(..) => restart = ui.add(primary_button("Restart now")).clicked(),
                    UpdateStatus::Idle | UpdateStatus::UpToDate => check = ui.button("Check now").clicked(),
                });
            });
            let last = self.settings.last_app_update_check;
            if let Some(t) = chrono::DateTime::from_timestamp(last, 0).filter(|_| last > 0) {
                let t = t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M");
                ui.label(RichText::new(format!("Last checked {t}")).small().color(TEXT_FAINT));
            }
            match &u.install {
                Install::AppImage(p) | Install::Binary(p) => hint(ui, &format!("Updates replace {}.", p.display())),
                Install::WindowsSetup => {
                    hint(ui, "Updates run the OpenDownloader installer when you quit or restart. No administrator rights needed.")
                }
                Install::Manual(why) => hint(
                    ui,
                    &format!("OpenDownloader can't install updates by itself because {why}. New versions are offered for download instead."),
                ),
            }
            if let UpdateStatus::Ready(_, staged) = &u.status {
                hint(ui, match staged {
                    update::Staged::Replaced => "The new version starts the next time you open OpenDownloader.",
                    update::Staged::Setup(_) => "The new version installs when you quit OpenDownloader.",
                });
            }
            if let Some(e) = &u.error {
                ui.add(egui::Label::new(RichText::new(e).small().color(DANGER)).wrap());
            }
            if let Some(r) = u.release().filter(|r| !r.notes.is_empty()) {
                ui.add_space(6.0);
                ui.label(RichText::new(format!("What's new in {}", r.version)).strong());
                egui::ScrollArea::vertical().id_salt("release-notes").max_height(200.0).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(&r.notes).color(TEXT_DIM)).wrap());
                });
                ui.hyperlink_to("Release page", &r.page);
            }
        });
        ui.add_space(10.0);
        section_title(ui, "Automatic updates");
        let s = &mut self.settings;
        setting_row(ui, "When a new version is released", None, |ui| {
            egui::ComboBox::from_id_salt("app-updates").selected_text(s.app_updates.label()).width(240.0).show_ui(ui, |ui| {
                for m in AppUpdates::ALL {
                    changed |= ui.selectable_value(&mut s.app_updates, m, m.label()).changed();
                }
            });
        });
        hint(ui, match s.app_updates {
            AppUpdates::Off => "OpenDownloader only checks when you click Check now or Help → Check for Updates.",
            AppUpdates::Notify => "Checks once a day and asks before updating.",
            AppUpdates::Install => "Checks once a day and installs new versions in the background; they take effect after a restart.",
        });
        let mut recheck = false;
        let prereleases = setting_row(
            ui,
            "Include pre-releases (beta versions)",
            Some("Newer features sooner, but they may have more bugs."),
            |ui| toggle(ui, &mut s.app_update_prereleases).changed(),
        );
        if prereleases {
            changed = true;
            recheck = true;
        }
        if !s.skipped_app_version.is_empty() {
            let mut unskip = false;
            setting_row(ui, &format!("Skipped version {}", s.skipped_app_version), None, |ui| {
                unskip = ui.button("Remind me again").clicked();
            });
            if unskip {
                s.skipped_app_version.clear();
                self.app_update.dismissed = false;
                changed = true;
            }
        }
        ui.add_space(4.0);
        hint(ui, "yt-dlp has its own automatic updates under Components.");
        if check || recheck {
            self.check_app_update(true);
        }
        if install {
            self.download_app_update();
        }
        if skip {
            self.skip_app_update();
        }
        if restart {
            let ctx = ui.ctx().clone();
            self.restart_to_update(&ctx, false);
        }
        changed
    }
}

fn paint_qr(ui: &mut egui::Ui, modules: &[bool], width: usize, size: f32) {
    let quiet = 3;
    let total = width + quiet * 2;
    let module = (size / total as f32).floor().max(1.0);
    let side = module * total as f32;
    let (rect, _) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(6), Color32::WHITE);
    for y in 0..width {
        for x in 0..width {
            if modules[y * width + x] {
                let min = rect.min + vec2((x + quiet) as f32 * module, (y + quiet) as f32 * module);
                painter.rect_filled(Rect::from_min_size(min, vec2(module, module)), CornerRadius::ZERO, Color32::BLACK);
            }
        }
    }
}
