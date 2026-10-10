//! Application state, background message handling and actions.

use crate::items::Items;
use crate::platform;
use crate::views::add::{AddDialog, AddFlow, Preselect};
use crate::views::convert::ConvertView;
use crate::views::settings::{SettingsPage, SettingsView};
use eframe::egui;
use odl_core::auth::{self, bilibili};
use odl_core::settings::AuthMethod;
use odl_core::tools::{self, InstallProgress, Tool, ToolSource};
use odl_core::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// Messages from background tasks to the UI thread.
pub enum UiMsg {
    Engine(EngineEvent),
    Probed { req: u64, result: Result<ProbeResult, String> },
    ToolVersion(Tool, Result<String, String>),
    ToolProgress(Tool, InstallProgress),
    ToolInstalled(Tool, Result<String, String>),
    LatestYtdlp(String),
    FolderPicked(PickTarget, PathBuf),
    FilesPicked(Vec<PathBuf>),
    CookieFilePicked(Site, PathBuf),
    ToolPathPicked(Tool, PathBuf),
    Bili(BiliMsg),
}

pub enum BiliMsg {
    Generated { url: String },
    Scanned,
    Expired,
    LoggedIn(Option<String>),
    Account(Result<Option<String>, String>),
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickTarget {
    DialogOutput,
    DefaultDir,
    SmartDir,
    ConvertOutput,
}

/// Sends messages to the UI and wakes it up.
#[derive(Clone)]
pub struct Bus {
    tx: Sender<UiMsg>,
    ctx: egui::Context,
}

impl Bus {
    pub fn send(&self, msg: UiMsg) {
        let _ = self.tx.send(msg);
        self.ctx.request_repaint();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    All,
    Video,
    Audio,
    Playlists,
    Converted,
}

impl Tab {
    pub const ALL: [Tab; 5] = [Tab::All, Tab::Video, Tab::Audio, Tab::Playlists, Tab::Converted];
    pub fn label(self) -> &'static str {
        match self {
            Tab::All => "All",
            Tab::Video => "Video",
            Tab::Audio => "Audio",
            Tab::Playlists => "Playlists",
            Tab::Converted => "Converted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

pub struct Toast {
    pub kind: ToastKind,
    pub text: String,
    pub created: Instant,
}

#[derive(Default)]
pub struct ToolState {
    pub version: Option<String>,
    pub busy: Option<String>,
    pub error: Option<String>,
}

pub enum Confirm {
    DeleteFile(ItemId),
    DeletePartial(ItemId),
    ClearList,
    Quit,
}

#[derive(Default)]
pub enum BiliLogin {
    #[default]
    Idle,
    Loading,
    Showing { modules: Vec<bool>, width: usize, scanned: bool },
    Expired,
    Error(String),
}

/// Something the user did in the list; applied after drawing.
#[derive(Debug, Clone)]
pub enum Action {
    Select(ItemId),
    EnterGroup(ItemId),
    Pause(ItemId),
    Resume(ItemId),
    Remove(ItemId),
    AskDeleteFile(ItemId),
    AskDeletePartial(ItemId),
    OpenFile(ItemId),
    ShowInFolder(ItemId),
    CopyLink(ItemId),
    CopyError(ItemId),
    OpenLink(ItemId),
    DownloadAgain(ItemId),
    Convert(ItemId, ConvertTarget),
    ConvertDialog(ItemId),
    Paste,
    ClearCompleted,
    AskClearList,
}

pub struct OdlApp {
    pub rt: tokio::runtime::Runtime,
    pub bus: Bus,
    rx: Receiver<UiMsg>,
    pub manager: Manager,
    pub settings: Settings,
    settings_dirty: bool,
    pub tools: ToolPaths,
    pub tool_state: HashMap<Tool, ToolState>,
    pub latest_ytdlp: Option<String>,
    pub items: Items,
    history_dirty: bool,
    last_save: Instant,

    pub tab: Tab,
    pub group_view: Option<ItemId>,
    pub search: String,
    pub selected: Option<ItemId>,

    pub add: Option<AddFlow>,
    probe_seq: u64,
    smart_probes: HashSet<u64>,
    pub paste_requested: Option<Instant>,

    pub settings_view: Option<SettingsView>,
    pub convert_view: Option<ConvertView>,
    pub confirm: Option<Confirm>,
    pub about_open: bool,
    pub toasts: Vec<Toast>,

    pub bili: BiliLogin,
    bili_cancel: Option<CancellationToken>,
    pub cookie_checks: HashMap<Site, Result<auth::CookieCheck, String>>,
    /// Removed downloads whose partial files are deleted once their process exits.
    pending_purge: HashMap<ItemId, Vec<PathBuf>>,
    pub cred_inputs: HashMap<Site, (String, String)>,
    quitting: bool,
}

impl OdlApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::fonts::install(&cc.egui_ctx);
        crate::theme::apply(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(3)
            .thread_name("odl-worker")
            .enable_all()
            .build()
            .expect("failed to start the async runtime");
        let (tx, rx) = std::sync::mpsc::channel();
        let bus = Bus { tx, ctx: cc.egui_ctx.clone() };

        let mut settings = store::load_settings();
        if settings.download_dir.as_os_str().is_empty() {
            settings.download_dir = odl_core::paths::default_download_dir();
        }
        let tools = tools::resolve(&settings);
        let sink_bus = bus.clone();
        let manager = Manager::spawn(
            rt.handle(),
            Arc::new(RunCtx { settings: settings.clone(), tools: tools.clone() }),
            Arc::new(move |e| sink_bus.send(UiMsg::Engine(e))),
        );
        let items = Items::new(store::load_history());

        let mut app = Self {
            rt,
            bus,
            rx,
            manager,
            settings,
            settings_dirty: false,
            tools,
            tool_state: HashMap::new(),
            latest_ytdlp: None,
            items,
            history_dirty: false,
            last_save: Instant::now(),
            tab: Tab::All,
            group_view: None,
            search: String::new(),
            selected: None,
            add: None,
            probe_seq: 0,
            smart_probes: HashSet::new(),
            paste_requested: None,
            settings_view: None,
            convert_view: None,
            confirm: None,
            about_open: false,
            toasts: Vec::new(),
            bili: BiliLogin::Idle,
            bili_cancel: None,
            cookie_checks: HashMap::new(),
            pending_purge: HashMap::new(),
            cred_inputs: HashMap::new(),
            quitting: false,
        };
        app.refresh_tool_versions();
        app.auto_update_ytdlp();
        // `opendownloader <url>` (e.g. from a browser or the desktop file) opens that link.
        if let Some(arg) = std::env::args().skip(1).find(|a| !a.starts_with('-')) {
            app.begin_link(&arg);
        }
        app
    }

    pub fn run_ctx(&self) -> RunCtx {
        RunCtx { settings: self.settings.clone(), tools: self.tools.clone() }
    }

    /// Call after any settings change: persists and applies to the engine.
    pub fn settings_changed(&mut self) {
        self.settings_dirty = true;
        self.manager.set_context(Arc::new(self.run_ctx()));
    }

    fn reresolve_tools(&mut self) {
        self.tools = tools::resolve(&self.settings);
        self.manager.set_context(Arc::new(self.run_ctx()));
        self.refresh_tool_versions();
    }

    pub fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toasts.push(Toast { kind, text: text.into(), created: Instant::now() });
        if self.toasts.len() > 5 {
            self.toasts.remove(0);
        }
    }

    pub fn missing_required_tools(&self) -> Vec<Tool> {
        [Tool::YtDlp, Tool::Ffmpeg].into_iter().filter(|t| self.tools.get(*t).is_none()).collect()
    }

    // -----------------------------------------------------------------------
    // Frame logic

    pub fn process_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.handle(msg);
        }
        if let Some(t) = self.paste_requested {
            if t.elapsed() > Duration::from_millis(1500) {
                self.paste_requested = None;
                self.toast(ToastKind::Info, "The clipboard doesn't contain a link. Copy one first.");
            }
        }
        let now = Instant::now();
        self.toasts.retain(|t| {
            let ttl = if t.kind == ToastKind::Error { 9 } else { 5 };
            now.duration_since(t.created) < Duration::from_secs(ttl)
        });
        self.autosave(false);
    }

    pub fn autosave(&mut self, force: bool) {
        if !force && self.last_save.elapsed() < Duration::from_secs(2) {
            return;
        }
        if self.history_dirty || force {
            if let Err(e) = store::save_history(&self.items.to_vec()) {
                tracing::error!("saving history: {e:#}");
            }
            self.history_dirty = false;
        }
        if self.settings_dirty || force {
            if let Err(e) = store::save_settings(&self.settings) {
                tracing::error!("saving settings: {e:#}");
            }
            self.settings_dirty = false;
        }
        self.last_save = Instant::now();
    }

    pub fn handle_close_request(&mut self, ctx: &egui::Context) {
        if self.quitting || !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if self.settings.confirm_exit_with_active && self.items.any_active() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Confirm::Quit);
        } else {
            self.quitting = true;
        }
    }

    pub fn quit(&mut self, ctx: &egui::Context) {
        self.quitting = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// Stops all jobs (they come back paused next time) and saves state.
    pub fn shutdown(&mut self) {
        self.manager.cancel_all();
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.items.any_active() && Instant::now() < deadline {
            match self.rx.recv_timeout(Duration::from_millis(100)) {
                Ok(msg) => self.handle(msg),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break,
            }
        }
        self.autosave(true);
    }

    fn handle(&mut self, msg: UiMsg) {
        match msg {
            UiMsg::Engine(ev) => self.handle_engine(ev),
            UiMsg::Probed { req, result } => self.handle_probe(req, result),
            UiMsg::ToolVersion(tool, r) => {
                let st = self.tool_state.entry(tool).or_default();
                match r {
                    Ok(v) => {
                        st.version = Some(v);
                        st.error = None;
                    }
                    Err(e) => {
                        st.version = None;
                        st.error = Some(e);
                    }
                }
            }
            UiMsg::ToolProgress(tool, p) => {
                self.tool_state.entry(tool).or_default().busy = Some(match p {
                    InstallProgress::Downloading { downloaded, total: Some(t) } if t > 0 => {
                        format!("Downloading… {:.0}% of {}", downloaded as f64 * 100.0 / t as f64, format_bytes(t))
                    }
                    InstallProgress::Downloading { downloaded, .. } => format!("Downloading… {}", format_bytes(downloaded)),
                    InstallProgress::Extracting => "Extracting…".into(),
                });
            }
            UiMsg::ToolInstalled(tool, r) => {
                let st = self.tool_state.entry(tool).or_default();
                st.busy = None;
                match r {
                    Ok(v) => {
                        st.error = None;
                        self.toast(ToastKind::Success, format!("{} {v} is ready", tool.label()));
                    }
                    Err(e) => {
                        st.error = Some(e.clone());
                        self.toast(ToastKind::Error, format!("{}: {e}", tool.label()));
                    }
                }
                self.reresolve_tools();
            }
            UiMsg::LatestYtdlp(v) => self.latest_ytdlp = Some(v),
            UiMsg::FolderPicked(target, path) => match target {
                PickTarget::DialogOutput => {
                    if let Some(AddFlow::Dialog(d)) = &mut self.add {
                        d.opts.output_dir = path;
                    }
                }
                PickTarget::DefaultDir => {
                    self.settings.download_dir = path.clone();
                    self.settings.last_options.output_dir = path;
                    self.settings_changed();
                }
                PickTarget::SmartDir => {
                    self.settings.smart_preset.output_dir = path;
                    self.settings_changed();
                }
                PickTarget::ConvertOutput => {
                    if let Some(v) = &mut self.convert_view {
                        v.output_dir = path;
                        v.same_folder = false;
                    }
                }
            },
            UiMsg::FilesPicked(files) => {
                let view = self.convert_view.get_or_insert_with(ConvertView::default);
                for f in files {
                    if !view.files.contains(&f) {
                        view.files.push(f);
                    }
                }
            }
            UiMsg::CookieFilePicked(site, path) => {
                let acct = self.settings.account_mut(site);
                acct.cookies_file = Some(path.clone());
                acct.method = AuthMethod::CookiesFile;
                self.cookie_checks.insert(site, auth::check_cookie_file(&path, site).map_err(|e| e.to_string()));
                self.settings_changed();
            }
            UiMsg::ToolPathPicked(tool, path) => {
                match tool {
                    Tool::YtDlp => self.settings.ytdlp_path = Some(path),
                    Tool::Ffmpeg => self.settings.ffmpeg_path = Some(path),
                    Tool::Deno => self.settings.deno_path = Some(path),
                }
                self.settings_changed();
                self.reresolve_tools();
            }
            UiMsg::Bili(m) => self.handle_bili(m),
        }
    }

    fn handle_engine(&mut self, ev: EngineEvent) {
        let id = match &ev {
            EngineEvent::Started(id)
            | EngineEvent::Progress(id, _)
            | EngineEvent::Finished(id, _)
            | EngineEvent::Failed(id, _)
            | EngineEvent::Cancelled(id) => *id,
            EngineEvent::TempFile(id, path) => {
                let list = match self.pending_purge.get_mut(id) {
                    Some(list) => list,
                    None => match self.items.get_mut(*id) {
                        Some(item) => &mut item.temp_files,
                        None => return,
                    },
                };
                if !list.contains(path) {
                    list.push(path.clone());
                    self.history_dirty = true;
                }
                return;
            }
        };
        // A download the user removed with "delete partial download": once it has stopped,
        // delete everything it wrote.
        if matches!(ev, EngineEvent::Finished(..) | EngineEvent::Failed(..) | EngineEvent::Cancelled(_)) {
            if let Some(mut files) = self.pending_purge.remove(&id) {
                if let EngineEvent::Finished(_, o) = &ev {
                    files.extend(o.file.clone());
                }
                self.finish_purge(&files);
                return;
            }
        }
        let Some(item) = self.items.get_mut(id) else { return };
        match ev {
            EngineEvent::TempFile(..) => {}
            EngineEvent::Started(_) => {
                item.status = ItemStatus::Running;
                item.error = None;
                item.progress = Progress::default();
            }
            EngineEvent::Progress(_, p) => {
                item.progress = p;
                return; // not worth persisting
            }
            EngineEvent::Finished(_, o) => {
                item.status = ItemStatus::Completed;
                item.progress.fraction = Some(1.0);
                item.progress.stage = None;
                item.file_path = o.file;
                item.temp_files.clear();
                item.file_size = o.size;
                item.ext = o.ext;
                item.height = o.height;
                item.fps = o.fps;
                item.note = o.note;
                item.finished_at = Some(chrono::Utc::now());
                let (title, parent, kind) = (item.title.clone(), item.parent, item.kind);
                if parent.is_none() {
                    let notify = match kind {
                        ItemKind::Conversion => self.settings.notify_conversion_finished,
                        _ => self.settings.notify_download_finished,
                    };
                    if notify {
                        let summary = if kind == ItemKind::Conversion { "Conversion finished" } else { "Download finished" };
                        platform::notify(summary, &title);
                    }
                    if self.settings.remove_completed {
                        self.items.remove(id);
                    }
                }
            }
            EngineEvent::Failed(_, msg) => {
                item.status = ItemStatus::Failed;
                item.progress.stage = None;
                item.progress.speed = None;
                item.error = Some(msg);
            }
            EngineEvent::Cancelled(_) => {
                item.status = ItemStatus::Paused;
                item.progress.speed = None;
                item.progress.eta = None;
                item.progress.stage = None;
            }
        }
        self.history_dirty = true;
        if let Some(parent) = self.items.get(id).and_then(|i| i.parent) {
            self.on_group_changed(parent);
        }
    }

    fn on_group_changed(&mut self, gid: ItemId) {
        let Some((old, new)) = self.items.refresh_group(gid) else { return };
        if old == new || new != ItemStatus::Completed {
            return;
        }
        let Some(group) = self.items.get(gid) else { return };
        let title = group.title.clone();
        let count = self.items.children_ids(gid).len();
        if self.settings.generate_m3u {
            self.write_m3u(gid);
        }
        if self.settings.notify_download_finished {
            platform::notify("Playlist downloaded", &format!("{title} ({count} items)"));
        }
        if self.settings.remove_completed {
            self.items.remove(gid);
            if self.group_view == Some(gid) {
                self.group_view = None;
            }
        }
    }

    fn write_m3u(&self, gid: ItemId) {
        let Some(group) = self.items.get(gid) else { return };
        let mut text = String::from("#EXTM3U\n");
        for id in self.items.children_ids(gid) {
            let Some(c) = self.items.get(id) else { continue };
            let Some(path) = &c.file_path else { continue };
            let name = path
                .strip_prefix(&group.output_dir)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| path.to_string_lossy().into_owned());
            text.push_str(&format!("#EXTINF:{},{}\n{name}\n", c.duration.unwrap_or(-1.0).round() as i64, c.title));
        }
        let path = group.output_dir.join(format!("{}.m3u", sanitize_filename(&group.title)));
        if let Err(e) = std::fs::write(&path, text) {
            tracing::warn!("writing {}: {e}", path.display());
        }
    }

    // -----------------------------------------------------------------------
    // Adding links

    /// Entry point for pasted text / the Paste Link button.
    pub fn begin_link(&mut self, text: &str) {
        let link = match parse_link(text) {
            Ok(l) => l,
            Err(e) => {
                self.toast(ToastKind::Error, e.to_string());
                return;
            }
        };
        if self.tools.ytdlp.is_none() {
            self.toast(ToastKind::Error, "yt-dlp is not installed yet. Install it in Settings → Components.");
            self.settings_view = Some(SettingsView { page: SettingsPage::Components });
            return;
        }
        let smart = self.settings.smart_mode;
        match &link.kind {
            LinkKind::VideoInPlaylist { .. } if !smart => self.add = Some(AddFlow::Choose(link)),
            LinkKind::VideoInPlaylist { .. } => {
                let url = link.video_only_url();
                self.probe(link, url, true, Preselect::All, true)
            }
            _ => {
                let url = link.url.clone();
                self.probe(link, url, false, Preselect::All, smart)
            }
        }
    }

    pub fn probe(&mut self, link: Link, url: String, no_playlist: bool, preselect: Preselect, smart: bool) {
        self.probe_seq += 1;
        let req = self.probe_seq;
        let ctx = self.run_ctx();
        let bus = self.bus.clone();
        let (site, probe_url) = (link.site, url.clone());
        self.rt.spawn(async move {
            let result = ytdlp::probe(&ctx, &probe_url, site, no_playlist).await.map_err(|e| format!("{e:#}"));
            bus.send(UiMsg::Probed { req, result });
        });
        if smart {
            self.smart_probes.insert(req);
            self.toast(ToastKind::Info, "Smart Mode: adding download…");
        } else {
            self.add = Some(AddFlow::Probing { req, link, url, preselect, started: Instant::now() });
        }
    }

    fn handle_probe(&mut self, req: u64, result: Result<ProbeResult, String>) {
        if self.smart_probes.remove(&req) {
            match result {
                Ok(probe) => {
                    let opts = self.settings.smart_options();
                    self.start_from_probe(&probe, None, None, opts);
                }
                Err(e) => self.toast(ToastKind::Error, e),
            }
            return;
        }
        let Some(AddFlow::Probing { req: pending, .. }) = &self.add else { return };
        if *pending != req {
            return;
        }
        let Some(AddFlow::Probing { link, preselect, .. }) = self.add.take() else { return };
        match result {
            Ok(probe) => {
                let dialog = AddDialog::new(link, probe, preselect, &self.settings);
                self.add = Some(AddFlow::Dialog(Box::new(dialog)));
            }
            Err(e) => self.add = Some(AddFlow::Error(e)),
        }
    }

    /// Creates download items for a probe result. `selected` limits playlist entries.
    pub fn start_from_probe(
        &mut self,
        probe: &ProbeResult,
        site: Option<Site>,
        selected: Option<&[bool]>,
        mut opts: DownloadOptions,
    ) {
        let site_of = |url: &str| parse_link(url).ok().map(|l| l.site);
        match probe {
            ProbeResult::Single(info) => {
                let Some(site) = site.or_else(|| site_of(&info.url)) else { return };
                if !site.has_video() || !info.has_video() {
                    opts.media = MediaKind::Audio;
                }
                let id = self.items.alloc_id();
                let mut item = DownloadItem::new(id, ItemKind::Media, &info.title, opts.output_dir.clone());
                item.site = Some(site);
                item.url = info.url.clone();
                item.uploader = info.uploader.clone();
                item.thumbnail = info.thumbnail.clone();
                item.duration = info.duration;
                item.options = Some(opts);
                self.enqueue_new(item);
                self.toast(ToastKind::Success, format!("Downloading “{}”", info.title));
            }
            ProbeResult::Playlist(pl) => {
                let entries: Vec<&PlaylistEntry> = pl
                    .entries
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| selected.is_none_or(|s| s.get(*i).copied().unwrap_or(false)))
                    .map(|(_, e)| e)
                    .collect();
                if entries.is_empty() {
                    return;
                }
                let Some(site) = site.or_else(|| site_of(&pl.url)).or_else(|| site_of(&entries[0].url)) else { return };
                if !site.has_video() {
                    opts.media = MediaKind::Audio;
                }
                let gid = self.items.alloc_id();
                let dir = if self.settings.playlist_subdirs {
                    opts.output_dir.join(sanitize_filename(&pl.title))
                } else {
                    opts.output_dir.clone()
                };
                let mut group = DownloadItem::new(gid, ItemKind::Group, &pl.title, dir.clone());
                group.site = Some(site);
                group.url = pl.url.clone();
                group.uploader = pl.uploader.clone();
                group.thumbnail = pl.thumbnail.clone().or_else(|| entries[0].thumbnail.clone());
                group.options = Some(opts.clone());
                self.items.insert(group);

                let width = entries.iter().map(|e| e.index).max().unwrap_or(1).to_string().len().max(2);
                for e in &entries {
                    let id = self.items.alloc_id();
                    let mut item = DownloadItem::new(id, ItemKind::Media, &e.title, dir.clone());
                    item.parent = Some(gid);
                    item.index = Some(e.index);
                    item.site = Some(site_of(&e.url).unwrap_or(site));
                    item.url = e.url.clone();
                    item.uploader = e.uploader.clone();
                    item.thumbnail = e.thumbnail.clone();
                    item.duration = e.duration;
                    item.options = Some(opts.clone());
                    if self.settings.numerate_playlist_files {
                        item.file_prefix = Some(format!("{:0width$}. ", e.index));
                    }
                    self.enqueue_new(item);
                }
                self.items.refresh_group(gid);
                self.toast(ToastKind::Success, format!("Downloading {} items from “{}”", entries.len(), pl.title));
            }
        }
    }

    fn enqueue_new(&mut self, item: DownloadItem) {
        if let Some(job) = item.job() {
            self.manager.enqueue(job);
        }
        self.items.insert(item);
        self.history_dirty = true;
    }

    // -----------------------------------------------------------------------
    // Item actions

    fn runnable_ids(&self, id: ItemId) -> Vec<ItemId> {
        match self.items.get(id) {
            Some(i) if i.kind == ItemKind::Group => self.items.children_ids(id),
            Some(_) => vec![id],
            None => vec![],
        }
    }

    pub fn pause(&mut self, id: ItemId) {
        for cid in self.runnable_ids(id) {
            if self.items.get(cid).is_some_and(|i| i.status.is_active()) {
                self.manager.cancel(cid);
            }
        }
    }

    pub fn resume(&mut self, id: ItemId) {
        for cid in self.runnable_ids(id) {
            let Some(item) = self.items.get_mut(cid) else { continue };
            if matches!(item.status, ItemStatus::Paused | ItemStatus::Failed) {
                if let Some(job) = item.job() {
                    item.status = ItemStatus::Queued;
                    item.error = None;
                    self.manager.enqueue(job);
                }
            }
        }
        if let Some(parent) = self.items.get(id).and_then(|i| i.parent) {
            self.items.refresh_group(parent);
        }
        self.items.refresh_group(id);
        self.history_dirty = true;
    }

    pub fn pause_all(&mut self) {
        self.manager.cancel_all();
    }

    pub fn resume_all(&mut self) {
        let ids: Vec<ItemId> =
            self.items.iter().filter(|i| i.kind != ItemKind::Group && i.status == ItemStatus::Paused).map(|i| i.id).collect();
        for id in ids {
            self.resume(id);
        }
    }

    pub fn remove(&mut self, id: ItemId) {
        for cid in self.runnable_ids(id) {
            if self.items.get(cid).is_some_and(|i| i.status.is_active()) {
                self.manager.cancel(cid);
            }
        }
        let parent = self.items.get(id).and_then(|i| i.parent);
        self.items.remove(id);
        if let Some(p) = parent {
            if self.items.children_ids(p).is_empty() {
                self.items.remove(p);
                self.group_view = None;
            } else {
                self.items.refresh_group(p);
            }
        }
        if self.group_view == Some(id) {
            self.group_view = None;
        }
        if self.selected == Some(id) {
            self.selected = None;
        }
        self.history_dirty = true;
    }

    /// Bytes of partial data on disk for an item (or a group's unfinished children).
    pub fn partial_size(&self, id: ItemId) -> u64 {
        self.runnable_ids(id)
            .iter()
            .filter_map(|c| self.items.get(*c))
            .filter(|i| i.status != ItemStatus::Completed)
            .flat_map(|i| i.temp_files.iter())
            .filter_map(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .sum()
    }

    /// Stops an unfinished download, removes it from the list and deletes its partial files.
    /// Finished items in a playlist keep their files.
    pub fn delete_partial(&mut self, id: ItemId) {
        let mut now = Vec::new();
        for cid in self.runnable_ids(id) {
            let Some(item) = self.items.get(cid) else { continue };
            match item.status {
                ItemStatus::Completed => {}
                ItemStatus::Running => {
                    // Files are still open; delete them when the process has exited.
                    self.pending_purge.insert(cid, item.temp_files.clone());
                    self.manager.cancel(cid);
                }
                _ => now.extend(item.temp_files.iter().cloned()),
            }
        }
        self.remove(id);
        self.finish_purge(&now);
    }

    fn finish_purge(&mut self, files: &[PathBuf]) {
        if files.is_empty() {
            return;
        }
        let (count, bytes) = ytdlp::delete_partial_files(files);
        if count > 0 {
            self.toast(ToastKind::Info, format!("Deleted partial download ({})", format_bytes(bytes)));
        }
    }

    pub fn delete_file(&mut self, id: ItemId) {
        let paths: Vec<PathBuf> =
            self.runnable_ids(id).iter().filter_map(|c| self.items.get(*c)).filter_map(|i| i.file_path.clone()).collect();
        let mut failed = 0;
        for p in &paths {
            if p.exists() && std::fs::remove_file(p).is_err() {
                failed += 1;
            }
        }
        if failed > 0 {
            self.toast(ToastKind::Error, format!("Could not delete {failed} file(s)"));
        } else {
            self.toast(ToastKind::Info, format!("Deleted {} file(s)", paths.len()));
        }
        self.remove(id);
    }

    pub fn clear_completed(&mut self) {
        let items = &self.items;
        let done: Vec<ItemId> = items
            .top_level()
            .filter(|i| i.status == ItemStatus::Completed)
            .map(|i| i.id)
            .collect();
        for id in done {
            self.items.remove(id);
        }
        self.group_view = None;
        self.history_dirty = true;
    }

    pub fn clear_list(&mut self) {
        self.manager.cancel_all();
        self.items = Items::default();
        self.group_view = None;
        self.selected = None;
        self.history_dirty = true;
    }

    pub fn start_conversion(&mut self, source: PathBuf, target: ConvertTarget, bitrate: AudioBitrate, out: Option<PathBuf>) {
        let id = self.items.alloc_id();
        let name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let dir = out.clone().or_else(|| source.parent().map(Path::to_path_buf)).unwrap_or_default();
        let mut item = DownloadItem::new(id, ItemKind::Conversion, format!("{name} → {}", target.label()), dir);
        item.convert = Some(ConvertOptions { source, target, bitrate, output_dir: out });
        self.enqueue_new(item);
    }

    pub fn apply_actions(&mut self, ctx: &egui::Context, actions: Vec<Action>) {
        for a in actions {
            match a {
                Action::Select(id) => self.selected = Some(id),
                Action::EnterGroup(id) => {
                    self.group_view = Some(id);
                    self.selected = None;
                }
                Action::Pause(id) => self.pause(id),
                Action::Resume(id) => self.resume(id),
                Action::Remove(id) => self.remove(id),
                Action::AskDeleteFile(id) => self.confirm = Some(Confirm::DeleteFile(id)),
                Action::AskDeletePartial(id) => self.confirm = Some(Confirm::DeletePartial(id)),
                Action::OpenFile(id) => {
                    if let Some(p) = self.items.get(id).and_then(|i| i.file_path.clone()) {
                        if p.exists() {
                            platform::open_path(&p);
                        } else {
                            self.toast(ToastKind::Error, "The file was moved or deleted.");
                        }
                    }
                }
                Action::ShowInFolder(id) => {
                    if let Some(i) = self.items.get(id) {
                        let target = i.file_path.clone().filter(|p| p.exists()).unwrap_or_else(|| i.output_dir.clone());
                        if target.exists() {
                            platform::show_in_folder(&target);
                        } else {
                            self.toast(ToastKind::Error, "The folder doesn't exist yet.");
                        }
                    }
                }
                Action::CopyLink(id) => {
                    if let Some(i) = self.items.get(id) {
                        ctx.copy_text(i.url.clone());
                        self.toast(ToastKind::Info, "Link copied");
                    }
                }
                Action::CopyError(id) => {
                    if let Some(e) = self.items.get(id).and_then(|i| i.error.clone()) {
                        ctx.copy_text(e);
                        self.toast(ToastKind::Info, "Error message copied");
                    }
                }
                Action::OpenLink(id) => {
                    if let Some(i) = self.items.get(id) {
                        platform::open_url(&i.url);
                    }
                }
                Action::DownloadAgain(id) => {
                    if let Some(url) = self.items.get(id).map(|i| i.url.clone()) {
                        self.begin_link(&url);
                    }
                }
                Action::Convert(id, target) => {
                    if let Some(p) = self.items.get(id).and_then(|i| i.file_path.clone()) {
                        self.start_conversion(p, target, AudioBitrate::Best, None);
                        self.toast(ToastKind::Info, format!("Converting to {}", target.label()));
                    }
                }
                Action::ConvertDialog(id) => {
                    let mut view = ConvertView::default();
                    if let Some(p) = self.items.get(id).and_then(|i| i.file_path.clone()) {
                        view.files.push(p);
                    }
                    self.convert_view = Some(view);
                }
                Action::Paste => self.request_paste(ctx),
                Action::ClearCompleted => self.clear_completed(),
                Action::AskClearList => self.confirm = Some(Confirm::ClearList),
            }
        }
    }

    pub fn request_paste(&mut self, ctx: &egui::Context) {
        self.paste_requested = Some(Instant::now());
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
    }

    // -----------------------------------------------------------------------
    // File pickers (async so the UI keeps rendering)

    pub fn pick_folder(&self, target: PickTarget, start: &Path) {
        let bus = self.bus.clone();
        let start = start.to_path_buf();
        self.rt.spawn(async move {
            let mut dialog = rfd::AsyncFileDialog::new().set_title("Choose a folder");
            if start.is_dir() {
                dialog = dialog.set_directory(&start);
            }
            if let Some(h) = dialog.pick_folder().await {
                bus.send(UiMsg::FolderPicked(target, h.path().to_path_buf()));
            }
        });
    }

    pub fn pick_media_files(&self) {
        let bus = self.bus.clone();
        let start = self.settings.download_dir.clone();
        self.rt.spawn(async move {
            let mut dialog = rfd::AsyncFileDialog::new().set_title("Choose media files").add_filter(
                "Media",
                &["mp4", "mkv", "webm", "mov", "avi", "flv", "m4a", "mp3", "ogg", "opus", "flac", "wav", "aac", "wma", "m4v", "ts"],
            );
            if start.is_dir() {
                dialog = dialog.set_directory(&start);
            }
            if let Some(files) = dialog.pick_files().await {
                bus.send(UiMsg::FilesPicked(files.into_iter().map(|f| f.path().to_path_buf()).collect()));
            }
        });
    }

    pub fn pick_cookie_file(&self, site: Site) {
        let bus = self.bus.clone();
        self.rt.spawn(async move {
            if let Some(h) = rfd::AsyncFileDialog::new()
                .set_title("Choose a cookies.txt file")
                .add_filter("Cookies", &["txt"])
                .pick_file()
                .await
            {
                bus.send(UiMsg::CookieFilePicked(site, h.path().to_path_buf()));
            }
        });
    }

    pub fn pick_tool_path(&self, tool: Tool) {
        let bus = self.bus.clone();
        self.rt.spawn(async move {
            if let Some(h) = rfd::AsyncFileDialog::new().set_title(format!("Locate {}", tool.label())).pick_file().await {
                bus.send(UiMsg::ToolPathPicked(tool, h.path().to_path_buf()));
            }
        });
    }

    // -----------------------------------------------------------------------
    // Components

    pub fn refresh_tool_versions(&mut self) {
        for tool in Tool::ALL {
            let Some(resolved) = self.tools.get(tool).cloned() else {
                self.tool_state.entry(tool).or_default().version = None;
                continue;
            };
            let bus = self.bus.clone();
            self.rt.spawn(async move {
                let r = tools::version(tool, &resolved.path).await.map_err(|e| format!("{e:#}"));
                bus.send(UiMsg::ToolVersion(tool, r));
            });
        }
    }

    pub fn install_tool(&mut self, tool: Tool) {
        let st = self.tool_state.entry(tool).or_default();
        if st.busy.is_some() {
            return;
        }
        st.busy = Some("Starting…".into());
        st.error = None;
        let bus = self.bus.clone();
        self.rt.spawn(async move {
            let pbus = bus.clone();
            let result = tools::install(tool, move |p| pbus.send(UiMsg::ToolProgress(tool, p))).await;
            let result = match result {
                Ok(path) => tools::version(tool, &path).await.map_err(|e| format!("{e:#}")),
                Err(e) => Err(format!("{e:#}")),
            };
            bus.send(UiMsg::ToolInstalled(tool, result));
        });
    }

    pub fn install_missing_tools(&mut self) {
        for tool in Tool::ALL {
            if self.tools.get(tool).is_none() {
                self.install_tool(tool);
            }
        }
    }

    pub fn update_ytdlp(&mut self) {
        let Some(resolved) = self.tools.ytdlp.clone() else {
            self.install_tool(Tool::YtDlp);
            return;
        };
        let st = self.tool_state.entry(Tool::YtDlp).or_default();
        if st.busy.is_some() {
            return;
        }
        st.busy = Some("Updating…".into());
        let bus = self.bus.clone();
        self.rt.spawn(async move {
            let pbus = bus.clone();
            let r = tools::update_ytdlp(&resolved, move |p| pbus.send(UiMsg::ToolProgress(Tool::YtDlp, p)))
                .await
                .map_err(|e| format!("{e:#}"));
            bus.send(UiMsg::ToolInstalled(Tool::YtDlp, r));
        });
    }

    pub fn check_ytdlp_latest(&self) {
        let bus = self.bus.clone();
        self.rt.spawn(async move {
            match tools::latest_ytdlp_version().await {
                Ok(v) => bus.send(UiMsg::LatestYtdlp(v)),
                Err(e) => tracing::info!("update check failed: {e:#}"),
            }
        });
    }

    /// Once a day, silently update a managed yt-dlp (sites change often; old versions break).
    fn auto_update_ytdlp(&mut self) {
        let now = chrono::Utc::now().timestamp();
        let Some(resolved) = self.tools.ytdlp.clone() else { return };
        if now - self.settings.last_update_check < 24 * 3600 {
            return;
        }
        self.settings.last_update_check = now;
        self.settings_dirty = true;
        let managed = resolved.source == ToolSource::Managed && self.settings.auto_update_ytdlp;
        let bus = self.bus.clone();
        self.rt.spawn(async move {
            let Ok(latest) = tools::latest_ytdlp_version().await else { return };
            bus.send(UiMsg::LatestYtdlp(latest.clone()));
            let Ok(current) = tools::version(Tool::YtDlp, &resolved.path).await else { return };
            if managed && tools::is_newer_version(&current, &latest) {
                let r = tools::update_ytdlp(&resolved, |_| {}).await.map_err(|e| format!("{e:#}"));
                bus.send(UiMsg::ToolInstalled(Tool::YtDlp, r));
            }
        });
    }

    // -----------------------------------------------------------------------
    // Bilibili QR login

    pub fn start_bili_login(&mut self) {
        if let Some(c) = self.bili_cancel.take() {
            c.cancel();
        }
        let cancel = CancellationToken::new();
        self.bili_cancel = Some(cancel.clone());
        self.bili = BiliLogin::Loading;
        let bus = self.bus.clone();
        self.rt.spawn(async move {
            let send = |m| bus.send(UiMsg::Bili(m));
            let result: anyhow::Result<()> = async {
                let client = bilibili::client()?;
                let qr = bilibili::generate(&client).await?;
                send(BiliMsg::Generated { url: qr.url.clone() });
                let mut scanned = false;
                loop {
                    tokio::select! {
                        _ = cancel.cancelled() => return Ok(()),
                        _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                    }
                    match bilibili::poll(&client, &qr.key).await? {
                        bilibili::PollState::Waiting => {}
                        bilibili::PollState::Scanned => {
                            if !scanned {
                                scanned = true;
                                send(BiliMsg::Scanned);
                            }
                        }
                        bilibili::PollState::Expired => {
                            send(BiliMsg::Expired);
                            return Ok(());
                        }
                        bilibili::PollState::Success { cookies } => {
                            let path = bilibili::save_cookies(&cookies)?;
                            let name = bilibili::account_name(&client, &path).await.ok().flatten();
                            send(BiliMsg::LoggedIn(name));
                            return Ok(());
                        }
                    }
                }
            }
            .await;
            if let Err(e) = result {
                send(BiliMsg::Error(format!("{e:#}")));
            }
        });
    }

    pub fn cancel_bili_login(&mut self) {
        if let Some(c) = self.bili_cancel.take() {
            c.cancel();
        }
        self.bili = BiliLogin::Idle;
    }

    pub fn refresh_bili_account(&self) {
        let bus = self.bus.clone();
        self.rt.spawn(async move {
            let path = auth::managed_cookie_file(Site::Bilibili);
            let r = async {
                let client = bilibili::client()?;
                bilibili::account_name(&client, &path).await
            }
            .await
            .map_err(|e| format!("{e:#}"));
            bus.send(UiMsg::Bili(BiliMsg::Account(r)));
        });
    }

    fn handle_bili(&mut self, m: BiliMsg) {
        match m {
            BiliMsg::Generated { url } => match qrcode::QrCode::new(url.as_bytes()) {
                Ok(code) => {
                    let width = code.width();
                    let modules = code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
                    self.bili = BiliLogin::Showing { modules, width, scanned: false };
                }
                Err(e) => self.bili = BiliLogin::Error(e.to_string()),
            },
            BiliMsg::Scanned => {
                if let BiliLogin::Showing { scanned, .. } = &mut self.bili {
                    *scanned = true;
                }
            }
            BiliMsg::Expired => self.bili = BiliLogin::Expired,
            BiliMsg::LoggedIn(name) => {
                self.bili = BiliLogin::Idle;
                self.bili_cancel = None;
                let acct = self.settings.account_mut(Site::Bilibili);
                acct.method = AuthMethod::QrCode;
                acct.display_name = name.clone();
                self.settings_changed();
                self.toast(
                    ToastKind::Success,
                    match name {
                        Some(n) => format!("Signed in to Bilibili as {n}"),
                        None => "Signed in to Bilibili".into(),
                    },
                );
            }
            BiliMsg::Account(r) => match r {
                Ok(Some(name)) => {
                    self.settings.account_mut(Site::Bilibili).display_name = Some(name);
                    self.settings_changed();
                }
                Ok(None) => {
                    self.settings.account_mut(Site::Bilibili).display_name = None;
                    self.settings_changed();
                    self.toast(ToastKind::Error, "Bilibili session expired. Sign in again.");
                }
                Err(e) => self.toast(ToastKind::Error, format!("Bilibili: {e}")),
            },
            BiliMsg::Error(e) => {
                self.bili_cancel = None;
                self.bili = BiliLogin::Error(e);
            }
        }
    }

    pub fn sign_out(&mut self, site: Site) {
        if let Err(e) = auth::sign_out(site) {
            self.toast(ToastKind::Error, format!("Sign out failed: {e:#}"));
        }
        let acct = self.settings.account_mut(site);
        acct.method = AuthMethod::None;
        acct.display_name = None;
        acct.username.clear();
        self.cookie_checks.remove(&site);
        self.settings_changed();
    }
}

impl eframe::App for OdlApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_messages();
        self.handle_close_request(ctx);
        if self.items.any_active() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        if !self.toasts.is_empty() || self.paste_requested.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.ui_root(ui);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.shutdown();
    }
}
