//! User settings (persisted as JSON).

use crate::model::{AudioFormat, DownloadOptions, MediaKind, Quality, VideoContainer};
use crate::site::Site;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // General
    pub download_dir: PathBuf,
    pub smart_mode: bool,
    /// Options used by Smart Mode (one-click downloads).
    pub smart_preset: DownloadOptions,
    /// Last options chosen in the download dialog, used as its defaults.
    pub last_options: DownloadOptions,
    pub confirm_exit_with_active: bool,

    // Downloads
    pub max_concurrent: usize,
    /// Parallel fragment downloads for HLS/DASH (`yt-dlp -N`).
    pub fragment_threads: u32,
    pub playlist_subdirs: bool,
    pub numerate_playlist_files: bool,
    pub skip_duplicates: bool,
    /// Record finished downloads in an archive and skip them next time.
    pub use_download_archive: bool,
    pub generate_m3u: bool,
    pub embed_subtitles: bool,
    pub embed_thumbnail: bool,
    pub embed_metadata: bool,
    pub embed_chapters: bool,
    /// Also keep the original danmaku/comment file (XML / JSON) next to the .ass.
    pub keep_raw_comments: bool,
    /// Cut SponsorBlock "sponsor" segments from YouTube videos.
    pub sponsorblock: bool,
    pub remove_completed: bool,
    pub filename_template: String,
    /// Extra raw arguments passed to yt-dlp (advanced).
    pub extra_ytdlp_args: String,

    // Connection
    /// Download rate limit in KiB/s, 0 = unlimited.
    pub rate_limit_kib: u32,
    pub proxy: ProxyConfig,
    /// Bilibili CDN mirror: "auto" (fastest), a host name, or empty to keep what Bilibili returns.
    pub bilibili_cdn: String,

    // Notifications
    pub notify_download_finished: bool,
    pub notify_conversion_finished: bool,

    // Accounts
    pub accounts: BTreeMap<Site, AccountConfig>,

    // Components
    pub ytdlp_path: Option<PathBuf>,
    pub ffmpeg_path: Option<PathBuf>,
    pub deno_path: Option<PathBuf>,
    pub auto_update_ytdlp: bool,
    /// Unix timestamp of the last automatic yt-dlp update check.
    pub last_update_check: i64,
}

impl Default for Settings {
    fn default() -> Self {
        let dir = crate::paths::default_download_dir();
        let preset = DownloadOptions { output_dir: dir.clone(), ..Default::default() };
        Self {
            download_dir: dir,
            smart_mode: false,
            smart_preset: preset.clone(),
            last_options: preset,
            confirm_exit_with_active: true,
            max_concurrent: 3,
            fragment_threads: 4,
            playlist_subdirs: true,
            numerate_playlist_files: true,
            skip_duplicates: true,
            use_download_archive: false,
            generate_m3u: false,
            embed_subtitles: true,
            embed_thumbnail: true,
            embed_metadata: true,
            embed_chapters: true,
            keep_raw_comments: false,
            sponsorblock: false,
            remove_completed: false,
            filename_template: "%(title).200B.%(ext)s".into(),
            extra_ytdlp_args: String::new(),
            rate_limit_kib: 0,
            proxy: ProxyConfig::default(),
            bilibili_cdn: crate::bilicdn::AUTO.into(),
            notify_download_finished: true,
            notify_conversion_finished: true,
            accounts: BTreeMap::new(),
            ytdlp_path: None,
            ffmpeg_path: None,
            deno_path: None,
            auto_update_ytdlp: true,
            last_update_check: 0,
        }
    }
}

impl Settings {
    pub fn account(&self, site: Site) -> AccountConfig {
        self.accounts.get(&site).cloned().unwrap_or_default()
    }

    pub fn account_mut(&mut self, site: Site) -> &mut AccountConfig {
        self.accounts.entry(site).or_default()
    }

    /// Default options for a new download dialog.
    pub fn dialog_defaults(&self) -> DownloadOptions {
        let mut o = self.last_options.clone();
        if o.output_dir.as_os_str().is_empty() {
            o.output_dir = self.download_dir.clone();
        }
        o
    }

    /// Smart Mode preset with a sane fallback.
    pub fn smart_options(&self) -> DownloadOptions {
        let mut o = self.smart_preset.clone();
        if o.output_dir.as_os_str().is_empty() {
            o.output_dir = self.download_dir.clone();
        }
        o
    }
}

/// A built-in preset; handy for the Smart Mode selector.
pub fn preset(media: MediaKind) -> DownloadOptions {
    match media {
        MediaKind::Video => DownloadOptions {
            media,
            container: VideoContainer::Mp4,
            quality: Quality::Height(1080),
            ..Default::default()
        },
        MediaKind::Audio => DownloadOptions { media, audio_format: AudioFormat::Mp3, ..Default::default() },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ProxyKind {
    #[default]
    Http,
    Https,
    Socks5,
}

impl ProxyKind {
    pub const ALL: [ProxyKind; 3] = [ProxyKind::Http, ProxyKind::Https, ProxyKind::Socks5];
    pub fn scheme(self) -> &'static str {
        match self {
            ProxyKind::Http => "http",
            ProxyKind::Https => "https",
            ProxyKind::Socks5 => "socks5",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ProxyConfig {
    pub enabled: bool,
    pub kind: ProxyKind,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

impl ProxyConfig {
    /// Proxy URL for yt-dlp's `--proxy`, or `None` when disabled/incomplete.
    pub fn url(&self) -> Option<String> {
        if !self.enabled || self.host.trim().is_empty() || self.port == 0 {
            return None;
        }
        let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
        let auth = if self.username.is_empty() {
            String::new()
        } else if self.password.is_empty() {
            format!("{}@", enc(&self.username))
        } else {
            format!("{}:{}@", enc(&self.username), enc(&self.password))
        };
        Some(format!("{}://{auth}{}:{}", self.kind.scheme(), self.host.trim(), self.port))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AuthMethod {
    #[default]
    None,
    /// Read cookies from an installed browser profile.
    Browser,
    /// A Netscape-format cookies.txt file.
    CookiesFile,
    /// Username + password (Niconico) or OAuth token (SoundCloud), stored in a netrc file.
    Password,
    /// Bilibili QR code login, cookies are stored by OpenDownloader.
    QrCode,
}

impl AuthMethod {
    pub fn label(self) -> &'static str {
        match self {
            AuthMethod::None => "Not signed in",
            AuthMethod::Browser => "Cookies from browser",
            AuthMethod::CookiesFile => "cookies.txt file",
            AuthMethod::Password => "Account login",
            AuthMethod::QrCode => "QR code login",
        }
    }

    /// Methods offered for a site.
    pub fn for_site(site: Site) -> &'static [AuthMethod] {
        match site {
            Site::YouTube => &[AuthMethod::None, AuthMethod::Browser, AuthMethod::CookiesFile],
            Site::Niconico => &[AuthMethod::None, AuthMethod::Password, AuthMethod::Browser, AuthMethod::CookiesFile],
            Site::Bilibili => &[AuthMethod::None, AuthMethod::QrCode, AuthMethod::Browser, AuthMethod::CookiesFile],
            Site::SoundCloud => &[AuthMethod::None, AuthMethod::Password, AuthMethod::Browser, AuthMethod::CookiesFile],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Browser {
    #[default]
    Firefox,
    Chrome,
    Chromium,
    Brave,
    Edge,
    Opera,
    Vivaldi,
}

impl Browser {
    pub const ALL: [Browser; 7] = [
        Browser::Firefox,
        Browser::Chrome,
        Browser::Chromium,
        Browser::Brave,
        Browser::Edge,
        Browser::Opera,
        Browser::Vivaldi,
    ];

    pub fn ytdlp_name(self) -> &'static str {
        match self {
            Browser::Firefox => "firefox",
            Browser::Chrome => "chrome",
            Browser::Chromium => "chromium",
            Browser::Brave => "brave",
            Browser::Edge => "edge",
            Browser::Opera => "opera",
            Browser::Vivaldi => "vivaldi",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Browser::Firefox => "Firefox",
            Browser::Chrome => "Google Chrome",
            Browser::Chromium => "Chromium",
            Browser::Brave => "Brave",
            Browser::Edge => "Microsoft Edge",
            Browser::Opera => "Opera",
            Browser::Vivaldi => "Vivaldi",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AccountConfig {
    pub method: AuthMethod,
    pub browser: Browser,
    /// Optional browser profile name or path.
    pub browser_profile: String,
    pub cookies_file: Option<PathBuf>,
    /// Login name shown in the UI for the password method (the secret lives in netrc).
    pub username: String,
    /// Display name of the signed-in account, when known (e.g. Bilibili nickname).
    pub display_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_url() {
        let mut p = ProxyConfig { enabled: true, host: "127.0.0.1".into(), port: 1080, ..Default::default() };
        p.kind = ProxyKind::Socks5;
        assert_eq!(p.url().as_deref(), Some("socks5://127.0.0.1:1080"));
        p.username = "me".into();
        p.password = "p@ss word".into();
        assert_eq!(p.url().as_deref(), Some("socks5://me:p%40ss+word@127.0.0.1:1080"));
        p.enabled = false;
        assert_eq!(p.url(), None);
    }

    #[test]
    fn settings_roundtrip_and_unknown_fields() {
        let mut s = Settings::default();
        s.account_mut(Site::Bilibili).method = AuthMethod::QrCode;
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.account(Site::Bilibili).method, AuthMethod::QrCode);
        // Old/partial files still load.
        let partial: Settings = serde_json::from_str(r#"{"max_concurrent": 5, "future_field": 1}"#).unwrap();
        assert_eq!(partial.max_concurrent, 5);
        assert!(partial.embed_metadata);
    }
}
