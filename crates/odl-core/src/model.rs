//! Core data types: download options, probe results and download list items.

use crate::site::Site;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub type ItemId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum MediaKind {
    #[default]
    Video,
    Audio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum VideoContainer {
    #[default]
    Mp4,
    Mkv,
    Webm,
}

impl VideoContainer {
    pub const ALL: [VideoContainer; 3] = [Self::Mp4, Self::Mkv, Self::Webm];

    pub fn ext(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mkv => "mkv",
            Self::Webm => "webm",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Mp4 => "MP4",
            Self::Mkv => "MKV",
            Self::Webm => "WEBM",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum AudioFormat {
    #[default]
    Mp3,
    M4a,
    Ogg,
    Opus,
    Flac,
    Wav,
}

impl AudioFormat {
    pub const ALL: [AudioFormat; 6] = [Self::Mp3, Self::M4a, Self::Ogg, Self::Opus, Self::Flac, Self::Wav];

    pub fn ext(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::M4a => "m4a",
            Self::Ogg => "ogg",
            Self::Opus => "opus",
            Self::Flac => "flac",
            Self::Wav => "wav",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Mp3 => "MP3",
            Self::M4a => "M4A",
            Self::Ogg => "OGG",
            Self::Opus => "OPUS",
            Self::Flac => "FLAC",
            Self::Wav => "WAV",
        }
    }

    /// Codec name understood by `yt-dlp --audio-format`.
    pub fn ytdlp_name(self) -> &'static str {
        match self {
            Self::Ogg => "vorbis",
            other => other.ext(),
        }
    }

    pub fn is_lossless(self) -> bool {
        matches!(self, Self::Flac | Self::Wav)
    }

    pub fn supports_cover_art(self) -> bool {
        !matches!(self, Self::Wav)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum VideoCodec {
    #[default]
    Auto,
    H264,
    Vp9,
    Av1,
}

impl VideoCodec {
    pub const ALL: [VideoCodec; 4] = [Self::Auto, Self::H264, Self::Vp9, Self::Av1];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::H264 => "H.264",
            Self::Vp9 => "VP9",
            Self::Av1 => "AV1",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Quality {
    Height(u32),
    Best,
}

impl Default for Quality {
    fn default() -> Self {
        Quality::Height(1080)
    }
}

pub const QUALITY_STEPS: [Quality; 10] = [
    Quality::Height(144),
    Quality::Height(240),
    Quality::Height(360),
    Quality::Height(480),
    Quality::Height(720),
    Quality::Height(1080),
    Quality::Height(1440),
    Quality::Height(2160),
    Quality::Height(4320),
    Quality::Best,
];

impl Quality {
    pub fn label(self) -> String {
        match self {
            Quality::Height(1440) => "2K".into(),
            Quality::Height(2160) => "4K".into(),
            Quality::Height(4320) => "8K".into(),
            Quality::Height(h) => format!("{h}p"),
            Quality::Best => "Best".into(),
        }
    }

    pub fn height(self) -> Option<u32> {
        match self {
            Quality::Height(h) => Some(h),
            Quality::Best => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum AudioBitrate {
    #[default]
    Best,
    Kbps(u32),
}

pub const AUDIO_BITRATES: [AudioBitrate; 7] = [
    AudioBitrate::Best,
    AudioBitrate::Kbps(320),
    AudioBitrate::Kbps(256),
    AudioBitrate::Kbps(192),
    AudioBitrate::Kbps(160),
    AudioBitrate::Kbps(128),
    AudioBitrate::Kbps(96),
];

impl AudioBitrate {
    pub fn label(self) -> String {
        match self {
            AudioBitrate::Best => "Best".into(),
            AudioBitrate::Kbps(k) => format!("{k} kbps"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadOptions {
    pub media: MediaKind,
    pub container: VideoContainer,
    pub audio_format: AudioFormat,
    pub quality: Quality,
    pub codec: VideoCodec,
    pub audio_bitrate: AudioBitrate,
    /// Comma separated subtitle languages (`en,ja`), `all`, or empty for none.
    pub subtitles: String,
    pub output_dir: PathBuf,
    /// Save danmaku (Bilibili) / comments (Niconico) as .ass subtitles.
    pub comments: bool,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            media: MediaKind::Video,
            container: VideoContainer::Mp4,
            audio_format: AudioFormat::Mp3,
            quality: Quality::default(),
            codec: VideoCodec::Auto,
            audio_bitrate: AudioBitrate::Best,
            subtitles: String::new(),
            output_dir: crate::paths::default_download_dir(),
            comments: false,
        }
    }
}

impl DownloadOptions {
    /// Short human description such as `MP4 · 1080p` or `MP3 · 320 kbps`.
    pub fn summary(&self) -> String {
        match self.media {
            MediaKind::Video => format!("{} · {}", self.container.label(), self.quality.label()),
            MediaKind::Audio if self.audio_format.is_lossless() => self.audio_format.label().to_string(),
            MediaKind::Audio => format!("{} · {}", self.audio_format.label(), self.audio_bitrate.label()),
        }
    }

    /// Typical total bitrate (kbit/s) of a download with these options. Used to estimate
    /// sizes when the site doesn't report them (playlist listings); errs on the high side.
    pub fn typical_kbps(&self) -> u64 {
        match self.media {
            MediaKind::Video => {
                let video = match self.quality.height().unwrap_or(1440) {
                    0..=144 => 120,
                    145..=240 => 300,
                    241..=360 => 600,
                    361..=480 => 1_100,
                    481..=720 => 2_200,
                    721..=1080 => 4_000,
                    1081..=1440 => 9_000,
                    1441..=2160 => 18_000,
                    _ => 45_000,
                };
                video + 160
            }
            MediaKind::Audio => match (self.audio_format, self.audio_bitrate) {
                (AudioFormat::Wav, _) => 1_411,
                (AudioFormat::Flac, _) => 950,
                (_, AudioBitrate::Kbps(k)) => k as u64,
                (AudioFormat::Mp3 | AudioFormat::Ogg, AudioBitrate::Best) => 260,
                (_, AudioBitrate::Best) => 160,
            },
        }
    }

    /// Estimated output size for media of the given duration.
    pub fn estimate_bytes(&self, duration_secs: f64) -> u64 {
        (duration_secs.max(0.0) * self.typical_kbps() as f64 * 125.0) as u64
    }

    /// Estimated (total, largest item) sizes for playlist entries. Entries without a known
    /// duration are assumed to be as long as the average known one (or 5 minutes).
    pub fn estimate_entries<'a>(&self, entries: impl IntoIterator<Item = &'a PlaylistEntry>) -> (u64, u64) {
        let durations: Vec<Option<f64>> = entries.into_iter().map(|e| e.duration).collect();
        let known: Vec<f64> = durations.iter().flatten().copied().collect();
        let fallback = if known.is_empty() { 300.0 } else { known.iter().sum::<f64>() / known.len() as f64 };
        durations.iter().fold((0, 0), |(total, largest), d| {
            let size = self.estimate_bytes(d.unwrap_or(fallback));
            (total + size, largest.max(size))
        })
    }

    /// Peak disk usage for the estimate: everything plus one extra copy of the largest
    /// video while it is being merged.
    pub fn peak_bytes(&self, total: u64, largest: u64) -> u64 {
        match self.media {
            MediaKind::Video => total + largest,
            MediaKind::Audio => total + largest / 2,
        }
    }

    pub fn wants_subtitles(&self) -> bool {
        self.media == MediaKind::Video && !self.subtitles.trim().is_empty()
    }
}

// ---------------------------------------------------------------------------
// Probe results

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MediaInfo {
    pub id: String,
    pub url: String,
    pub title: String,
    pub uploader: Option<String>,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
    /// Available video heights, ascending. Empty for audio-only media.
    pub heights: Vec<u32>,
    /// Approximate size of the best video stream per available height.
    pub video_sizes: Vec<(u32, u64)>,
    pub audio_size: Option<u64>,
    pub subtitle_langs: Vec<String>,
    pub is_live: bool,
}

impl MediaInfo {
    pub fn has_video(&self) -> bool {
        !self.heights.is_empty()
    }

    /// Rough output size for the given options, if the site reported sizes.
    pub fn estimate_size(&self, opts: &DownloadOptions) -> Option<u64> {
        match opts.media {
            MediaKind::Audio => match (opts.audio_format, opts.audio_bitrate, self.duration) {
                (f, _, Some(d)) if f == AudioFormat::Wav => Some((d * 176_400.0) as u64),
                (_, AudioBitrate::Kbps(k), Some(d)) => Some((d * k as f64 * 125.0) as u64),
                _ => self.audio_size,
            },
            MediaKind::Video => {
                let limit = opts.quality.height().unwrap_or(u32::MAX);
                let video = self
                    .video_sizes
                    .iter()
                    .filter(|(h, _)| *h <= limit)
                    .max_by_key(|(h, _)| *h)
                    .or_else(|| self.video_sizes.iter().min_by_key(|(h, _)| *h))
                    .map(|(_, s)| *s)?;
                Some(video + self.audio_size.unwrap_or(0))
            }
        }
    }

    /// The height that will actually be downloaded for the requested quality.
    pub fn effective_height(&self, quality: Quality) -> Option<u32> {
        let limit = quality.height().unwrap_or(u32::MAX);
        self.heights
            .iter()
            .copied()
            .filter(|h| *h <= limit)
            .max()
            .or_else(|| self.heights.first().copied())
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaylistEntry {
    /// 1-based position in the playlist.
    pub index: u32,
    pub id: String,
    pub url: String,
    pub title: String,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
    pub uploader: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaylistInfo {
    pub id: String,
    pub url: String,
    pub title: String,
    pub uploader: Option<String>,
    pub thumbnail: Option<String>,
    pub entries: Vec<PlaylistEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProbeResult {
    Single(MediaInfo),
    Playlist(PlaylistInfo),
}

// ---------------------------------------------------------------------------
// Download list items

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemKind {
    /// A single video/track download.
    Media,
    /// A playlist/collection; its children are `Media` items with `parent` set.
    Group,
    /// A local file conversion (Media Tools).
    Conversion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemStatus {
    Queued,
    Running,
    Paused,
    Completed,
    Failed,
}

impl ItemStatus {
    pub fn is_active(self) -> bool {
        matches!(self, ItemStatus::Queued | ItemStatus::Running)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Progress {
    /// 0.0..=1.0, `None` when unknown.
    pub fraction: Option<f32>,
    pub downloaded: u64,
    pub total: Option<u64>,
    /// Bytes per second.
    pub speed: Option<f64>,
    /// Seconds remaining.
    pub eta: Option<u64>,
    /// Post-processing stage, e.g. "Merging…".
    pub stage: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConvertTarget {
    Audio(AudioFormat),
    /// Re-encode to H.264/AAC MP4 (maximum compatibility).
    Mp4,
    /// Lossless remux into MKV.
    Mkv,
}

impl ConvertTarget {
    pub const ALL: [ConvertTarget; 8] = [
        ConvertTarget::Audio(AudioFormat::Mp3),
        ConvertTarget::Audio(AudioFormat::M4a),
        ConvertTarget::Audio(AudioFormat::Ogg),
        ConvertTarget::Audio(AudioFormat::Opus),
        ConvertTarget::Audio(AudioFormat::Flac),
        ConvertTarget::Audio(AudioFormat::Wav),
        ConvertTarget::Mp4,
        ConvertTarget::Mkv,
    ];

    pub fn ext(self) -> &'static str {
        match self {
            ConvertTarget::Audio(f) => f.ext(),
            ConvertTarget::Mp4 => "mp4",
            ConvertTarget::Mkv => "mkv",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ConvertTarget::Audio(f) => f.label(),
            ConvertTarget::Mp4 => "MP4 (H.264)",
            ConvertTarget::Mkv => "MKV (remux)",
        }
    }

    pub fn is_audio(self) -> bool {
        matches!(self, ConvertTarget::Audio(_))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConvertOptions {
    pub source: PathBuf,
    pub target: ConvertTarget,
    pub bitrate: AudioBitrate,
    /// Defaults to the source file's folder.
    pub output_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadItem {
    pub id: ItemId,
    #[serde(default)]
    pub parent: Option<ItemId>,
    pub kind: ItemKind,
    #[serde(default)]
    pub site: Option<Site>,
    #[serde(default)]
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub uploader: Option<String>,
    #[serde(default)]
    pub thumbnail: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub options: Option<DownloadOptions>,
    #[serde(default)]
    pub convert: Option<ConvertOptions>,
    /// Position inside the parent playlist.
    #[serde(default)]
    pub index: Option<u32>,
    /// Literal filename prefix, e.g. `03. ` for numbered playlist items.
    #[serde(default)]
    pub file_prefix: Option<String>,
    pub output_dir: PathBuf,
    pub status: ItemStatus,
    #[serde(skip)]
    pub progress: Progress,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub file_path: Option<PathBuf>,
    /// Partial files of an unfinished download (deleted by "Remove and Delete Partial Download").
    #[serde(default)]
    pub temp_files: Vec<PathBuf>,
    #[serde(default)]
    pub file_size: Option<u64>,
    #[serde(default)]
    pub ext: Option<String>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub fps: Option<f32>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub finished_at: Option<DateTime<Utc>>,
}

impl DownloadItem {
    pub fn new(id: ItemId, kind: ItemKind, title: impl Into<String>, output_dir: PathBuf) -> Self {
        Self {
            id,
            parent: None,
            kind,
            site: None,
            url: String::new(),
            title: title.into(),
            uploader: None,
            thumbnail: None,
            duration: None,
            options: None,
            convert: None,
            index: None,
            file_prefix: None,
            output_dir,
            status: ItemStatus::Queued,
            progress: Progress::default(),
            error: None,
            note: None,
            file_path: None,
            temp_files: Vec::new(),
            file_size: None,
            ext: None,
            height: None,
            fps: None,
            created_at: Utc::now(),
            finished_at: None,
        }
    }

    pub fn is_audio(&self) -> bool {
        match self.kind {
            ItemKind::Conversion => self.convert.as_ref().is_some_and(|c| c.target.is_audio()),
            _ => self.options.as_ref().is_some_and(|o| o.media == MediaKind::Audio),
        }
    }

    /// The job that (re)starts this item, if it is runnable.
    pub fn job(&self) -> Option<crate::Job> {
        use crate::manager::{DownloadJob, Job, JobSpec};
        let spec = match self.kind {
            ItemKind::Media => JobSpec::Download(DownloadJob {
                url: self.url.clone(),
                site: self.site?,
                options: self.options.clone()?,
                output_dir: self.output_dir.clone(),
                file_prefix: self.file_prefix.clone(),
                work_key: format!("{}-{}", self.id, self.created_at.timestamp_millis()),
            }),
            ItemKind::Conversion => JobSpec::Convert(self.convert.clone()?),
            ItemKind::Group => return None,
        };
        Some(Job { id: self.id, spec })
    }
}

// ---------------------------------------------------------------------------
// Subscriptions

/// A playlist/channel that is checked periodically; new entries are downloaded automatically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: u64,
    pub url: String,
    pub site: Site,
    pub title: String,
    #[serde(default)]
    pub uploader: Option<String>,
    #[serde(default)]
    pub thumbnail: Option<String>,
    /// Options for new downloads (output folder = the base folder; the playlist
    /// subfolder is added as for normal playlist downloads).
    pub options: DownloadOptions,
    /// Entry ids (or URLs) already known; anything else is new.
    #[serde(default)]
    pub seen: std::collections::BTreeSet<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub last_checked: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_new: usize,
    #[serde(default)]
    pub total_downloaded: usize,
    #[serde(default)]
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
}

fn default_true() -> bool {
    true
}

impl Subscription {
    /// Key used to recognize an entry across checks.
    pub fn entry_key(e: &PlaylistEntry) -> String {
        if e.id.is_empty() {
            e.url.clone()
        } else {
            e.id.clone()
        }
    }

    /// Entries of a fresh listing that haven't been seen before (in playlist order).
    pub fn new_entries<'a>(&self, pl: &'a PlaylistInfo) -> Vec<&'a PlaylistEntry> {
        pl.entries.iter().filter(|e| !self.seen.contains(&Self::entry_key(e))).collect()
    }

    pub fn is_due(&self, interval_hours: u32, now: DateTime<Utc>) -> bool {
        if !self.enabled || interval_hours == 0 {
            return false;
        }
        match self.last_checked {
            None => true,
            Some(t) => now - t >= chrono::Duration::hours(interval_hours as i64),
        }
    }
}

// ---------------------------------------------------------------------------
// Formatting helpers shared by the UI

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn format_duration(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

pub fn format_eta(seconds: u64) -> String {
    match seconds {
        0..=59 => format!("{seconds}s left"),
        60..=3599 => format!("{}m {}s left", seconds / 60, seconds % 60),
        _ => format!("{}h {}m left", seconds / 3600, (seconds % 3600) / 60),
    }
}

/// Makes a string safe to use as a single path component on Linux and Windows.
pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches(['.', ' ']).trim_start_matches('.');
    let mut out: String = trimmed.chars().take(150).collect();
    if out.is_empty() {
        out = "Untitled".into();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(45 * 1024 * 1024 + 200 * 1024), "45.2 MB");
        assert_eq!(format_duration(312.0), "05:12");
        assert_eq!(format_duration(3725.0), "1:02:05");
        assert_eq!(format_eta(7), "7s left");
    }

    #[test]
    fn sanitize() {
        assert_eq!(sanitize_filename("MOIRA - カラオケ"), "MOIRA - カラオケ");
        assert_eq!(sanitize_filename("a/b\\c:d*e?"), "a_b_c_d_e_");
        assert_eq!(sanitize_filename("..."), "Untitled");
        assert_eq!(sanitize_filename(" name. "), "name");
    }

    #[test]
    fn playlist_size_estimates() {
        let video = DownloadOptions { quality: Quality::Height(1080), ..Default::default() };
        // 1080p ≈ 4.16 Mbit/s ≈ 520 KB/s → a 10-minute video ≈ 312 MB.
        assert_eq!(video.estimate_bytes(600.0), 312_000_000);
        let entry = |d: Option<f64>| PlaylistEntry { duration: d, ..Default::default() };
        let entries = [entry(Some(600.0)), entry(Some(300.0)), entry(None)];
        let (total, largest) = video.estimate_entries(&entries);
        // The unknown one counts as the 450 s average.
        assert_eq!(total, video.estimate_bytes(1350.0));
        assert_eq!(largest, video.estimate_bytes(600.0));
        assert_eq!(video.peak_bytes(total, largest), total + largest);
        let mp3 = DownloadOptions { media: MediaKind::Audio, audio_bitrate: AudioBitrate::Kbps(320), ..Default::default() };
        assert_eq!(mp3.estimate_bytes(60.0), 2_400_000);
        assert_eq!(DownloadOptions { media: MediaKind::Audio, audio_format: AudioFormat::Wav, ..Default::default() }.typical_kbps(), 1_411);
    }

    #[test]
    fn subscription_new_entries_and_due() {
        let entry = |id: &str| PlaylistEntry { id: id.into(), url: format!("https://x/{id}"), ..Default::default() };
        let pl = PlaylistInfo { entries: vec![entry("a"), entry("b"), entry("c")], ..Default::default() };
        let now = Utc::now();
        let mut sub = Subscription {
            id: 1,
            url: "https://www.youtube.com/playlist?list=PL1".into(),
            site: Site::YouTube,
            title: "t".into(),
            uploader: None,
            thumbnail: None,
            options: DownloadOptions::default(),
            seen: ["a".to_string(), "b".to_string()].into(),
            enabled: true,
            last_checked: None,
            last_new: 0,
            total_downloaded: 0,
            last_error: None,
            created_at: now,
        };
        let new: Vec<&str> = sub.new_entries(&pl).iter().map(|e| e.id.as_str()).collect();
        assert_eq!(new, vec!["c"]);
        assert!(sub.is_due(6, now));
        sub.last_checked = Some(now - chrono::Duration::hours(2));
        assert!(!sub.is_due(6, now));
        assert!(sub.is_due(1, now));
        assert!(!sub.is_due(0, now));
        sub.enabled = false;
        assert!(!sub.is_due(1, now));
    }

    #[test]
    fn estimate_picks_best_height_under_limit() {
        let info = MediaInfo {
            heights: vec![360, 720, 1080],
            video_sizes: vec![(360, 10), (720, 20), (1080, 40)],
            audio_size: Some(5),
            ..Default::default()
        };
        let mut o = DownloadOptions { quality: Quality::Height(720), ..Default::default() };
        assert_eq!(info.estimate_size(&o), Some(25));
        o.quality = Quality::Best;
        assert_eq!(info.estimate_size(&o), Some(45));
        o.quality = Quality::Height(144);
        assert_eq!(info.estimate_size(&o), Some(15));
        assert_eq!(info.effective_height(Quality::Height(2160)), Some(1080));
    }
}
