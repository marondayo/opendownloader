//! yt-dlp integration: metadata probing, argument construction and progress parsing.

use crate::auth::AuthArgs;
use crate::manager::{DownloadJob, JobOutcome};
use crate::model::*;
use crate::process::{command, kill_tree, strip_ansi, LineReader};
use crate::settings::Settings;
use crate::site::Site;
use crate::tools::ToolSource;
use crate::{JobError, RunCtx};
use anyhow::{anyhow, Context};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

const PROGRESS_TAG: &str = "[odl] ";
const POST_TAG: &str = "[odlpp] ";
const SIZE_TAG: &str = "[odlsize] ";
const FILE_TAG: &str = "[odlfile] ";
/// Separates file paths in progress lines (ASCII unit separator, never in file names).
const PATH_SEP: char = '\u{1f}';

static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

fn run_tag() -> String {
    format!("{}-{}", std::process::id(), RUN_COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// yt-dlp command with the options shared by every invocation.
fn base_command(ctx: &RunCtx) -> anyhow::Result<Command> {
    let ytdlp = ctx
        .tools
        .ytdlp
        .as_ref()
        .ok_or_else(|| anyhow!("yt-dlp is not installed. Open Settings → Components to install it."))?;
    let mut cmd = command(&ytdlp.path);
    cmd.arg("--ignore-config");
    cmd.args(common_args(ctx));
    Ok(cmd)
}

/// Network/tool options applied to both probing and downloading.
pub fn common_args(ctx: &RunCtx) -> Vec<String> {
    let s = &ctx.settings;
    let mut args = Vec::new();
    if let Some(ff) = &ctx.tools.ffmpeg {
        if ff.source != ToolSource::System {
            args.push("--ffmpeg-location".into());
            args.push(ff.path.to_string_lossy().into_owned());
        }
    }
    if let Some(deno) = &ctx.tools.deno {
        if deno.source != ToolSource::System {
            args.push("--js-runtimes".into());
            args.push(format!("deno:{}", deno.path.to_string_lossy()));
        }
    }
    if let Some(proxy) = s.proxy.url() {
        args.push("--proxy".into());
        args.push(proxy);
    }
    args
}

/// Extra options for YouTube extraction, given the installed PO-token provider (if usable).
///
/// yt-dlp asks every listed client and merges their formats. `default` keeps yt-dlp's own,
/// maintained choice; `web_embedded` (the embed player) still serves full formats when the
/// default clients get "confirm you're not a bot". `mweb` needs a PO token, so it is only
/// added with the provider, and it covers videos whose embedding is disabled.
pub fn youtube_args(pot: Option<&crate::pot::PotProvider>) -> Vec<String> {
    let mut args = Vec::new();
    let clients = match pot {
        Some(p) => {
            args.extend(p.ytdlp_args());
            "default,web_embedded,mweb"
        }
        None => "default,web_embedded",
    };
    args.push("--extractor-args".into());
    args.push(format!("youtube:player_client={clients}"));
    args
}

/// [`youtube_args`] for YouTube links, nothing for other sites.
fn site_args(ctx: &RunCtx, site: Site) -> Vec<String> {
    if site != Site::YouTube {
        return Vec::new();
    }
    // The provider's script runs on Deno.
    let pot = ctx.tools.deno.as_ref().and_then(|_| crate::pot::installed());
    let mut args = youtube_args(pot.as_ref());
    // A yt-dlp from PATH (e.g. pip without yt-dlp-ejs) may lack the scripts that solve YouTube's
    // JavaScript challenges; official builds bundle them, and yt-dlp only fetches when needed.
    if ctx.tools.ytdlp.as_ref().is_some_and(|t| t.source != ToolSource::Managed) {
        args.extend(["--remote-components".to_string(), "ejs:github".to_string()]);
    }
    args
}

// ---------------------------------------------------------------------------
// Probing

/// Fetches metadata for a link. Playlists are listed flat (no per-entry requests).
pub async fn probe(ctx: &RunCtx, url: &str, site: Site, no_playlist: bool) -> anyhow::Result<ProbeResult> {
    let mut cmd = base_command(ctx)?;
    let auth = AuthArgs::prepare(site, &ctx.settings.account(site), &run_tag())?;
    cmd.args(site_args(ctx, site))
        .args(["-J", "--flat-playlist", "--no-warnings"])
        .arg(if no_playlist { "--no-playlist" } else { "--yes-playlist" })
        .args(&auth.args)
        .arg("--")
        .arg(url);
    let output = cmd.output().await.context("could not start yt-dlp")?;
    let signed_out = auth.signed_out.clone();
    drop(auth);
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let lines: Vec<String> = stderr.lines().map(strip_ansi).collect();
        return Err(anyhow!(explain_error(&lines, site, signed_out.as_deref())));
    }
    let json: Value = serde_json::from_slice(&output.stdout).context("yt-dlp returned invalid JSON")?;
    let mut result = parse_probe(&json)?;
    if ctx.settings.skip_duplicates {
        if let ProbeResult::Playlist(p) = &mut result {
            let mut seen = std::collections::HashSet::new();
            p.entries.retain(|e| e.id.is_empty() || seen.insert(e.id.clone()));
        }
    }
    Ok(result)
}

fn str_field(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str))
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

fn num_field(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

/// Picks a thumbnail around list-row size, falling back to the main thumbnail.
fn pick_thumbnail(v: &Value) -> Option<String> {
    if let Some(list) = v.get("thumbnails").and_then(Value::as_array) {
        let mut sized: Vec<(u64, String)> = list
            .iter()
            .filter_map(|t| {
                let url = t.get("url")?.as_str()?.to_string();
                let w = t.get("width")?.as_u64()?;
                Some((w, url))
            })
            .collect();
        sized.sort_by_key(|(w, _)| *w);
        if let Some((_, url)) = sized.iter().find(|(w, _)| *w >= 240).or(sized.last()) {
            return Some(fix_scheme(url));
        }
    }
    str_field(v, &["thumbnail"]).map(|u| fix_scheme(&u))
}

fn fix_scheme(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("//") {
        format!("https://{rest}")
    } else if let Some(rest) = url.strip_prefix("http://") {
        format!("https://{rest}")
    } else {
        url.to_string()
    }
}

pub fn parse_probe(v: &Value) -> anyhow::Result<ProbeResult> {
    let ty = v.get("_type").and_then(Value::as_str).unwrap_or("video");
    if ty == "playlist" || ty == "multi_video" {
        let entries = v
            .get("entries")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter(|e| !e.is_null())
                    .enumerate()
                    .filter_map(|(i, e)| parse_entry(e, i as u32 + 1))
                    .collect()
            })
            .unwrap_or_default();
        return Ok(ProbeResult::Playlist(PlaylistInfo {
            id: str_field(v, &["id"]).unwrap_or_default(),
            url: str_field(v, &["webpage_url", "original_url"]).unwrap_or_default(),
            title: str_field(v, &["title"]).unwrap_or_else(|| "Playlist".into()),
            uploader: str_field(v, &["uploader", "channel"]),
            thumbnail: pick_thumbnail(v),
            entries,
        }));
    }
    Ok(ProbeResult::Single(parse_media(v)))
}

fn parse_entry(e: &Value, position: u32) -> Option<PlaylistEntry> {
    let id = str_field(e, &["id"]).unwrap_or_default();
    let ie = str_field(e, &["ie_key"]).unwrap_or_default();
    let url = str_field(e, &["url", "webpage_url", "original_url"])
        .map(|u| {
            if u.starts_with("http") {
                u
            } else if ie.starts_with("Youtube") {
                format!("https://www.youtube.com/watch?v={u}")
            } else if ie.starts_with("Niconico") {
                format!("https://www.nicovideo.jp/watch/{u}")
            } else {
                u
            }
        })
        .filter(|u| u.starts_with("http"))?;
    Some(PlaylistEntry {
        index: e.get("playlist_index").and_then(Value::as_u64).map(|n| n as u32).unwrap_or(position),
        id,
        url,
        title: str_field(e, &["title"]).unwrap_or_else(|| format!("Item {position}")),
        duration: num_field(e, "duration"),
        thumbnail: pick_thumbnail(e),
        uploader: str_field(e, &["uploader", "channel"]),
    })
}

fn format_size(f: &Value) -> Option<u64> {
    f.get("filesize").and_then(Value::as_u64).or_else(|| f.get("filesize_approx").and_then(Value::as_u64))
}

fn is_none_codec(f: &Value, key: &str) -> bool {
    f.get(key).and_then(Value::as_str) == Some("none")
}

pub fn parse_media(v: &Value) -> MediaInfo {
    let formats: Vec<&Value> = v.get("formats").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();

    // Video streams: anything with a height whose vcodec isn't "none".
    let mut by_height: std::collections::BTreeMap<u32, (f64, Option<u64>)> = Default::default();
    for f in &formats {
        if is_none_codec(f, "vcodec") {
            continue;
        }
        let Some(h) = f.get("height").and_then(Value::as_u64).map(|h| h as u32).filter(|h| *h > 0) else {
            continue;
        };
        let tbr = num_field(f, "tbr").unwrap_or(0.0);
        let entry = by_height.entry(h).or_insert((-1.0, None));
        if tbr > entry.0 {
            *entry = (tbr, format_size(f));
        }
    }
    if by_height.is_empty() {
        if let Some(h) = v.get("height").and_then(Value::as_u64).filter(|h| *h > 0) {
            by_height.insert(h as u32, (0.0, format_size(v)));
        }
    }
    let heights: Vec<u32> = by_height.keys().copied().collect();
    let video_sizes: Vec<(u32, u64)> = by_height.iter().filter_map(|(h, (_, s))| s.map(|s| (*h, s))).collect();

    let audio_size = formats
        .iter()
        .filter(|f| is_none_codec(f, "vcodec") && !is_none_codec(f, "acodec"))
        .max_by(|a, b| {
            let ka = num_field(a, "abr").or_else(|| num_field(a, "tbr")).unwrap_or(0.0);
            let kb = num_field(b, "abr").or_else(|| num_field(b, "tbr")).unwrap_or(0.0);
            ka.total_cmp(&kb)
        })
        .and_then(|f| format_size(f))
        .or_else(|| if heights.is_empty() { format_size(v) } else { None });

    let mut subtitle_langs: Vec<String> = v
        .get("subtitles")
        .and_then(Value::as_object)
        .map(|o| o.keys().filter(|k| *k != "live_chat" && *k != "danmaku").cloned().collect())
        .unwrap_or_default();
    subtitle_langs.sort();

    MediaInfo {
        id: str_field(v, &["id"]).unwrap_or_default(),
        url: str_field(v, &["webpage_url", "original_url"]).unwrap_or_default(),
        title: str_field(v, &["title", "fulltitle"]).unwrap_or_else(|| "Untitled".into()),
        uploader: str_field(v, &["uploader", "channel", "creator"]),
        duration: num_field(v, "duration"),
        thumbnail: pick_thumbnail(v),
        heights,
        video_sizes,
        audio_size,
        subtitle_langs,
        is_live: v.get("is_live").and_then(Value::as_bool).unwrap_or(false),
    }
}

// ---------------------------------------------------------------------------
// Download arguments

/// Builds the yt-dlp arguments for a download job (excluding auth, tools and the URL).
pub fn download_args(settings: &Settings, job: &DownloadJob) -> Vec<String> {
    let o = &job.options;
    let mut a: Vec<String> = Vec::new();
    let mut push = |items: &[&str]| a.extend(items.iter().map(|s| s.to_string()));

    push(&["--newline", "--progress", "--no-simulate", "--no-playlist", "--no-mtime"]);
    push(&["--progress-template", &format!(
        "download:{PROGRESS_TAG}%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s|%(progress.fragment_index)s|%(progress.fragment_count)s|%(info.format_id)s|%(progress.tmpfilename)s{PATH_SEP}%(progress.filename)s"
    )]);
    push(&["--progress-template", &format!("postprocess:{POST_TAG}%(progress.status)s|%(progress.postprocessor)s")]);
    push(&["--print", &format!("video:{SIZE_TAG}%(filesize,filesize_approx)s")]);
    push(&["--print", &format!("after_move:{FILE_TAG}%(height)s|%(fps)s|%(ext)s|%(filepath)s")]);

    if settings.fragment_threads > 1 {
        push(&["-N", &settings.fragment_threads.to_string()]);
    }
    if settings.rate_limit_kib > 0 {
        push(&["-r", &format!("{}K", settings.rate_limit_kib)]);
    }

    match o.media {
        MediaKind::Video => {
            let mut sort: Vec<String> = Vec::new();
            if let Some(h) = o.quality.height() {
                sort.push(format!("res:{h}"));
            }
            let codec_pref = match (o.codec, o.container) {
                (VideoCodec::H264, _) | (VideoCodec::Auto, VideoContainer::Mp4) => Some("vcodec:h264"),
                (VideoCodec::Vp9, _) | (VideoCodec::Auto, VideoContainer::Webm) => Some("vcodec:vp9"),
                (VideoCodec::Av1, _) => Some("vcodec:av01"),
                (VideoCodec::Auto, VideoContainer::Mkv) => None,
            };
            if let Some(c) = codec_pref {
                sort.push(c.into());
            }
            match o.container {
                VideoContainer::Mp4 => sort.push("acodec:aac".into()),
                VideoContainer::Webm => sort.push("acodec:opus".into()),
                VideoContainer::Mkv => {}
            }
            push(&["-f", "bv*+ba/b"]);
            if !sort.is_empty() {
                push(&["-S", &sort.join(",")]);
            }
            let (merge, remux) = match o.container {
                VideoContainer::Mp4 => ("mp4", "mp4"),
                VideoContainer::Mkv => ("mkv", "mkv"),
                VideoContainer::Webm => ("webm/mkv", "webm>webm/mkv"),
            };
            push(&["--merge-output-format", merge, "--remux-video", remux]);
            if o.wants_subtitles() {
                push(&["--write-subs", "--sub-langs", o.subtitles.trim()]);
                if settings.embed_subtitles {
                    push(&["--embed-subs"]);
                }
            }
            if settings.embed_thumbnail && o.container != VideoContainer::Webm {
                push(&["--embed-thumbnail"]);
            }
            if settings.embed_chapters {
                push(&["--embed-chapters"]);
            }
        }
        MediaKind::Audio => {
            let f = o.audio_format;
            let selector = match f {
                AudioFormat::M4a => "ba[ext=m4a]/ba/b",
                AudioFormat::Opus => "ba[acodec=opus]/ba/b",
                _ => "ba/b",
            };
            push(&["-f", selector, "-x", "--audio-format", f.ytdlp_name()]);
            let quality = match (f.is_lossless(), o.audio_bitrate) {
                (true, _) | (false, AudioBitrate::Best) => "0".to_string(),
                (false, AudioBitrate::Kbps(k)) => format!("{k}K"),
            };
            push(&["--audio-quality", &quality]);
            if settings.embed_thumbnail && f.supports_cover_art() {
                push(&["--embed-thumbnail", "--convert-thumbnails", "jpg"]);
            }
        }
    }
    if settings.embed_metadata {
        push(&["--embed-metadata"]);
    }
    if settings.sponsorblock && job.site == Site::YouTube && o.media == MediaKind::Video {
        push(&["--sponsorblock-remove", "sponsor"]);
    }
    if settings.use_download_archive {
        push(&["--download-archive", &crate::paths::get().archive_file().to_string_lossy()]);
    }

    let template = if settings.filename_template.trim().is_empty() {
        "%(title).200B.%(ext)s".to_string()
    } else {
        settings.filename_template.trim().to_string()
    };
    let template = match &job.file_prefix {
        // `%` would be interpreted by yt-dlp's template engine.
        Some(prefix) => format!("{}{template}", prefix.replace('%', "%%")),
        None => template,
    };
    push(&["-P", &job.work_dir().to_string_lossy(), "-o", &template]);

    a.extend(split_args(&settings.extra_ytdlp_args));
    a
}

/// Splits a user-supplied argument string, honoring single and double quotes.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut has_token = false;
    for c in s.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                has_token = true;
            }
            (None, c) if c.is_whitespace() => {
                if has_token || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if has_token || !cur.is_empty() {
        out.push(cur);
    }
    out
}

// ---------------------------------------------------------------------------
// Output parsing

#[derive(Debug, Clone, PartialEq)]
pub struct RawProgress {
    pub status: String,
    pub downloaded: Option<u64>,
    pub total: Option<u64>,
    pub total_estimate: Option<u64>,
    pub speed: Option<f64>,
    pub eta: Option<u64>,
    pub fragment_index: Option<u64>,
    pub fragment_count: Option<u64>,
    pub format_id: String,
    /// Files yt-dlp is writing for this stream (`….part` and the stream's own file).
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileInfo {
    pub height: Option<u32>,
    pub fps: Option<f32>,
    pub ext: Option<String>,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OutLine {
    Progress(RawProgress),
    PostProcess { status: String, name: String },
    ExpectedSize(Option<u64>),
    File(FileInfo),
    Other(String),
}

fn na<T: std::str::FromStr>(s: &str) -> Option<T> {
    let s = s.trim();
    if s.is_empty() || s == "NA" || s == "None" {
        None
    } else {
        // yt-dlp prints numbers like "1234.0" for some float-typed fields.
        s.parse().ok().or_else(|| s.parse::<f64>().ok().and_then(|f| f.trunc().to_string().parse().ok()))
    }
}

pub fn parse_line(line: &str) -> OutLine {
    if let Some(rest) = line.strip_prefix(PROGRESS_TAG) {
        // Paths come last (they may contain '|') and are separated by PATH_SEP.
        let f: Vec<&str> = rest.splitn(10, '|').collect();
        if f.len() >= 9 {
            let files = f
                .get(9)
                .map(|p| p.split(PATH_SEP).filter(|s| !s.is_empty() && *s != "NA").map(PathBuf::from).collect())
                .unwrap_or_default();
            return OutLine::Progress(RawProgress {
                files,
                status: f[0].to_string(),
                downloaded: na(f[1]),
                total: na(f[2]),
                total_estimate: na(f[3]),
                speed: na(f[4]),
                eta: na(f[5]),
                fragment_index: na(f[6]),
                fragment_count: na(f[7]),
                format_id: if f[8] == "NA" { String::new() } else { f[8].to_string() },
            });
        }
    } else if let Some(rest) = line.strip_prefix(POST_TAG) {
        if let Some((status, name)) = rest.split_once('|') {
            return OutLine::PostProcess { status: status.into(), name: name.into() };
        }
    } else if let Some(rest) = line.strip_prefix(SIZE_TAG) {
        return OutLine::ExpectedSize(na(rest));
    } else if let Some(rest) = line.strip_prefix(FILE_TAG) {
        let f: Vec<&str> = rest.splitn(4, '|').collect();
        if f.len() == 4 {
            return OutLine::File(FileInfo {
                height: na(f[0]),
                fps: na(f[1]),
                ext: na(f[2]),
                path: PathBuf::from(f[3]),
            });
        }
    }
    OutLine::Other(line.to_string())
}

pub fn postprocessor_label(name: &str) -> &'static str {
    match name {
        "Merger" => "Merging video and audio…",
        "ExtractAudio" => "Converting audio…",
        "EmbedThumbnail" => "Embedding cover art…",
        "FFmpegMetadata" | "Metadata" => "Writing metadata…",
        "EmbedSubtitle" => "Embedding subtitles…",
        "VideoRemuxer" | "Remux" => "Remuxing…",
        "VideoConvertor" => "Converting video…",
        "SponsorBlock" | "ModifyChapters" => "Removing sponsor segments…",
        "FixupM3u8" | "FixupM4a" | "FixupStretched" | "FixupDuplicateMoov" | "FixupTimestamp" | "FixupDuration" => {
            "Fixing container…"
        }
        "ThumbnailsConvertor" => "Preparing cover art…",
        "MoveFiles" => "Finishing…",
        _ => "Processing…",
    }
}

/// Folds per-stream progress (video, then audio) into one overall progress value.
#[derive(Debug, Default)]
pub struct ProgressTracker {
    expected_total: Option<u64>,
    finished_bytes: u64,
    current_format: Option<String>,
    current_total: Option<u64>,
    current_downloaded: u64,
    current_finished: bool,
}

impl ProgressTracker {
    pub fn set_expected_total(&mut self, total: Option<u64>) {
        self.expected_total = total.filter(|t| *t > 0);
    }

    pub fn update(&mut self, p: &RawProgress) -> Progress {
        if self.current_format.as_deref() != Some(p.format_id.as_str()) {
            if self.current_format.is_some() && !self.current_finished {
                self.finished_bytes += self.current_total.unwrap_or(self.current_downloaded);
            }
            self.current_format = Some(p.format_id.clone());
            self.current_finished = false;
            self.current_total = None;
            self.current_downloaded = 0;
        }
        let total = p.total.or(p.total_estimate);
        if total.is_some() {
            self.current_total = total;
        }
        if let Some(d) = p.downloaded {
            self.current_downloaded = d;
        }
        if p.status == "finished" && !self.current_finished {
            self.finished_bytes += self.current_total.unwrap_or(self.current_downloaded).max(self.current_downloaded);
            self.current_finished = true;
        }
        let (downloaded, known_total) = if self.current_finished {
            (self.finished_bytes, self.finished_bytes)
        } else {
            (
                self.finished_bytes + self.current_downloaded,
                self.finished_bytes + self.current_total.unwrap_or(0),
            )
        };
        let total = match self.expected_total {
            Some(e) if e >= known_total => Some(e),
            _ if known_total > 0 => Some(known_total),
            _ => None,
        };
        let mut fraction = total.map(|t| (downloaded as f64 / t as f64).clamp(0.0, 1.0) as f32);
        if fraction.is_none() {
            if let (Some(i), Some(n)) = (p.fragment_index, p.fragment_count.filter(|n| *n > 0)) {
                fraction = Some((i as f32 / n as f32).clamp(0.0, 1.0));
            }
        }
        Progress {
            fraction,
            downloaded,
            total,
            speed: p.speed,
            eta: p.eta,
            stage: None,
        }
    }
}

/// Turns yt-dlp's stderr into a short, actionable message.
pub fn friendly_error(stderr_lines: &[String], site: Site) -> String {
    explain_error(stderr_lines, site, None)
}

/// [`friendly_error`] for a run that skipped sign-in (`signed_out` says why): a failure that needs
/// an account then explains the skipped sign-in instead of asking the user to sign in.
pub fn explain_error(stderr_lines: &[String], site: Site, signed_out: Option<&str>) -> String {
    let error = stderr_lines
        .iter()
        .rev()
        .find(|l| l.starts_with("ERROR:"))
        .map(|l| l.trim_start_matches("ERROR:").trim().to_string())
        .or_else(|| stderr_lines.iter().rev().find(|l| !l.trim().is_empty()).cloned())
        .unwrap_or_else(|| "yt-dlp failed without an error message".into());
    let lower = error.to_lowercase();
    let rotated = stderr_lines.iter().any(|l| l.to_lowercase().contains("cookies are no longer valid"));
    let hint = if let Some(reason) = signed_out.filter(|_| needs_account(&lower)) {
        Some(format!("Tip: this needs a signed-in account, but {reason}. Settings → Accounts shows how to fix it."))
    } else if lower.contains("could not copy chrome cookie database") {
        Some("Tip: the browser is open and locks its cookies. Close it, or use a cookies.txt file (Settings → Accounts).".into())
    } else if lower.contains("dpapi") || lower.contains("app-bound") {
        Some("Tip: this browser's cookies can't be read on Windows. Use Firefox or a cookies.txt file (Settings → Accounts).".into())
    } else if rotated {
        Some(format!("Tip: your {} cookies have expired. Export them again (Settings → Accounts).", site.name()))
    } else if site == Site::Bilibili && lower.contains("412") {
        Some(
            "Tip: Bilibili blocks anonymous requests from some networks (HTTP 412). \
             Sign in with the QR code in Settings → Accounts."
                .into(),
        )
    } else if lower.contains("sign in to confirm")
        || lower.contains("login required")
        || lower.contains("members-only")
        || lower.contains("private video")
        || lower.contains("premium")
        || lower.contains("account")
        || lower.contains("cookies")
    {
        Some(format!("Tip: sign in to {} in Settings → Accounts.", site.name()))
    } else if lower.contains("requested format is not available") {
        Some("Tip: try a different quality or format.".into())
    } else if lower.contains("ffmpeg") && (lower.contains("not found") || lower.contains("not installed")) {
        Some("Tip: install FFmpeg in Settings → Components.".into())
    } else if lower.contains("http error 403") || lower.contains("unable to extract") || lower.contains("nsig") {
        Some("Tip: updating yt-dlp (Settings → Components) usually fixes this.".into())
    } else if lower.contains("geo") && lower.contains("restrict") {
        Some("Tip: this content is region-locked; a proxy may help (Settings → Connection).".into())
    } else {
        None
    };
    match hint {
        Some(h) => format!("{error}\n{h}"),
        None => error,
    }
}

/// Whether a yt-dlp error means the video needs a signed-in account.
fn needs_account(lower: &str) -> bool {
    [
        "sign in",
        "login required",
        "members-only",
        "join this channel",
        "private video",
        "age-restricted",
        "confirm your age",
        "inappropriate for some users",
    ]
    .iter()
    .any(|k| lower.contains(k))
}

// ---------------------------------------------------------------------------
// Running a download

/// A temporary file deleted on drop.
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Extracts a Bilibili video, points its streams at the chosen mirror and saves the info JSON.
async fn prepare_bilibili(ctx: &RunCtx, job: &DownloadJob, auth: &AuthArgs) -> anyhow::Result<TempFile> {
    let mut cmd = base_command(ctx)?;
    cmd.args(["-J", "--no-playlist", "--no-warnings"]).args(&auth.args).arg("--").arg(&job.url);
    let out = cmd.output().await?;
    if !out.status.success() {
        let lines: Vec<String> = String::from_utf8_lossy(&out.stderr).lines().map(strip_ansi).collect();
        anyhow::bail!(friendly_error(&lines, Site::Bilibili));
    }
    let mut info: Value = serde_json::from_slice(&out.stdout)?;
    let choice = ctx.settings.bilibili_cdn.trim();
    let host = if choice == crate::bilicdn::AUTO {
        let sample = crate::bilicdn::sample_url(&info).ok_or_else(|| anyhow!("no mirrorable stream"))?;
        let (host, speed) =
            crate::bilicdn::fastest_mirror(&sample).await.ok_or_else(|| anyhow!("no mirror responded"))?;
        tracing::info!("bilibili: using {host} ({:.0} KB/s in test)", speed / 1024.0);
        host
    } else {
        choice.to_string()
    };
    crate::bilicdn::rewrite_hosts(&mut info, &host);
    let dir = crate::paths::get().temp_dir();
    tokio::fs::create_dir_all(&dir).await?;
    let path = dir.join(format!("bilibili-{}.info.json", run_tag()));
    tokio::fs::write(&path, serde_json::to_vec(&info)?).await?;
    Ok(TempFile(path))
}

/// Fetches danmaku (Bilibili) or comments (Niconico) and writes `<video stem>.danmaku.ass`
/// / `<video stem>.comments.ass` next to the video. Returns the number of comments.
async fn save_comments(ctx: &RunCtx, job: &DownloadJob, auth: &AuthArgs, video: &std::path::Path) -> anyhow::Result<usize> {
    use crate::danmaku::{self, AssConfig};
    use crate::settings::AuthMethod;
    let dir = video.parent().ok_or_else(|| anyhow!("no folder"))?;
    let stem = video.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let keep_raw = ctx.settings.keep_raw_comments;
    let (comments, ass_cfg, suffix) = match job.site {
        Site::Bilibili => {
            let account = ctx.settings.account(Site::Bilibili);
            let cookie_file = match account.method {
                AuthMethod::QrCode => Some(crate::auth::managed_cookie_file(Site::Bilibili)),
                AuthMethod::CookiesFile => account.cookies_file.clone(),
                _ => None,
            };
            let (cid, elems) = danmaku::fetch_bilibili(&job.url, cookie_file.as_deref()).await?;
            if keep_raw && !elems.is_empty() {
                std::fs::write(dir.join(format!("{stem}.danmaku.xml")), danmaku::bilibili_to_xml(cid, &elems))?;
            }
            (danmaku::bilibili_to_comments(&elems), AssConfig::bilibili(), "danmaku")
        }
        Site::Niconico => {
            // yt-dlp knows how to talk to Niconico's comment server (thread keys etc.).
            let tmp = crate::paths::get().temp_dir().join(format!("comments-{}", run_tag()));
            tokio::fs::create_dir_all(&tmp).await?;
            let mut cmd = base_command(ctx)?;
            cmd.args(["--skip-download", "--write-subs", "--sub-langs", "comments", "--no-playlist", "-P"])
                .arg(&tmp)
                .args(["-o", "c.%(ext)s"])
                .args(&auth.args)
                .arg("--")
                .arg(&job.url);
            let out = cmd.output().await?;
            let raw = tokio::fs::read(tmp.join("c.comments.json")).await;
            let _ = tokio::fs::remove_dir_all(&tmp).await;
            let raw = match raw {
                Ok(r) => r,
                Err(_) => {
                    let lines: Vec<String> = String::from_utf8_lossy(&out.stderr).lines().map(strip_ansi).collect();
                    anyhow::bail!(friendly_error(&lines, Site::Niconico));
                }
            };
            if keep_raw {
                std::fs::write(dir.join(format!("{stem}.comments.json")), &raw)?;
            }
            let json: Value = serde_json::from_slice(&raw)?;
            (danmaku::parse_niconico(&json), AssConfig::niconico(), "comments")
        }
        _ => return Ok(0),
    };
    if !comments.is_empty() {
        std::fs::write(dir.join(format!("{stem}.{suffix}.ass")), danmaku::to_ass(&comments, &ass_cfg))?;
    }
    Ok(comments.len())
}

/// `dir/name`, or `dir/stem (1).ext`, `dir/stem (2).ext`, … if that's taken.
pub fn unique_path(dir: &std::path::Path, file_name: &std::ffi::OsStr) -> PathBuf {
    let candidate = dir.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let p = std::path::Path::new(file_name);
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (1..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|c| !c.exists())
        .expect("unbounded range")
}

/// Moves a finished file into `dest_dir` under a free name; returns the new path.
fn move_out(path: &std::path::Path, dest_dir: &std::path::Path) -> std::io::Result<PathBuf> {
    let name = path.file_name().ok_or_else(|| std::io::Error::other("file has no name"))?;
    let dest = unique_path(dest_dir, name);
    if std::fs::rename(path, &dest).is_err() {
        std::fs::copy(path, &dest)?;
        std::fs::remove_file(path)?;
    }
    Ok(dest)
}

/// Moves leftover finished files out of a working folder and removes it.
fn finish_work_dir(work: &std::path::Path, dest_dir: &std::path::Path) {
    if let Ok(entries) = std::fs::read_dir(work) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let partial = name.ends_with(".part") || name.ends_with(".ytdl") || name.contains(".part-Frag");
            if e.path().is_file() && !partial {
                let _ = move_out(&e.path(), dest_dir);
            }
        }
    }
    let _ = std::fs::remove_dir_all(work);
}

/// Deletes the partial files of an unfinished download: the given paths plus yt-dlp's
/// `.ytdl` resume state and `-FragN` fragment files. Returns (files deleted, bytes freed).
pub fn delete_partial_files(paths: &[PathBuf]) -> (usize, u64) {
    let mut doomed: Vec<PathBuf> = Vec::new();
    for p in paths {
        doomed.push(p.clone());
        let mut ytdl = p.clone().into_os_string();
        ytdl.push(".ytdl");
        doomed.push(ytdl.into());
        if let (Some(dir), Some(name)) = (p.parent(), p.file_name().map(|n| n.to_string_lossy().into_owned())) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                let frag_prefix = format!("{name}-Frag");
                for e in entries.flatten() {
                    if e.file_name().to_string_lossy().starts_with(&frag_prefix) {
                        doomed.push(e.path());
                    }
                }
            }
        }
    }
    doomed.sort();
    doomed.dedup();
    let (mut count, mut bytes) = (0, 0);
    for p in &doomed {
        if let Ok(meta) = std::fs::metadata(p) {
            if meta.is_file() && std::fs::remove_file(p).is_ok() {
                count += 1;
                bytes += meta.len();
            }
        }
    }
    // The private working folder holds only this download's files (thumbnails, subtitles…).
    let mut work_dirs: Vec<&std::path::Path> = paths
        .iter()
        .filter_map(|p| p.parent())
        .filter(|d| d.file_name().is_some_and(|n| n.to_string_lossy().starts_with(".odl-partial-")))
        .collect();
    work_dirs.dedup();
    for d in work_dirs {
        let _ = std::fs::remove_dir_all(d);
    }
    (count, bytes)
}

/// Runs one download. `on_file` reports every file yt-dlp starts writing (for cleanup).
pub async fn download(
    ctx: &RunCtx,
    job: &DownloadJob,
    cancel: CancellationToken,
    on_progress: &(dyn Fn(Progress) + Send + Sync),
    on_file: &(dyn Fn(PathBuf) + Send + Sync),
) -> Result<JobOutcome, JobError> {
    tokio::fs::create_dir_all(&job.output_dir)
        .await
        .map_err(|e| JobError::Failed(format!("cannot create {}: {e}", job.output_dir.display())))?;
    if ctx.tools.ffmpeg.is_none() {
        return Err(JobError::Failed("FFmpeg is not installed. Open Settings → Components to install it.".into()));
    }
    let auth = AuthArgs::prepare(job.site, &ctx.settings.account(job.site), &run_tag())?;

    // Bilibili: move the streams to the fastest mirror (see `bilicdn`). Falls back to a
    // normal download if anything goes wrong.
    let mut info_file: Option<TempFile> = None;
    if job.site == Site::Bilibili && !ctx.settings.bilibili_cdn.trim().is_empty() {
        on_progress(Progress { stage: Some("Finding the fastest Bilibili server…".into()), ..Default::default() });
        tokio::select! {
            _ = cancel.cancelled() => return Err(JobError::Cancelled),
            r = prepare_bilibili(ctx, job, &auth) => match r {
                Ok(f) => info_file = Some(f),
                Err(e) => tracing::warn!("bilibili mirror selection skipped: {e:#}"),
            }
        }
    }

    let mut cmd = base_command(ctx)?;
    // Before `download_args`, so a player_client in the user's extra arguments wins.
    cmd.args(site_args(ctx, job.site)).args(download_args(&ctx.settings, job)).args(&auth.args);
    match &info_file {
        Some(f) => cmd.arg("--load-info-json").arg(&f.0),
        None => cmd.arg("--").arg(&job.url),
    };

    // Never fill the disk: refuse to start below the keep-free limit, check the expected
    // size once yt-dlp reports it, and keep watching while downloading/merging.
    let mut guard = crate::disk::SpaceGuard::new(&job.output_dir, ctx.settings.keep_free_mib);
    if let Some(g) = guard.as_mut() {
        g.poll().map_err(JobError::Failed)?;
    }
    let mut space_error: Option<String> = None;

    let mut child = cmd.spawn().map_err(|e| JobError::Failed(format!("could not start yt-dlp: {e}")))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");

    let stderr_task = tokio::spawn(async move {
        let mut reader = LineReader::new(stderr);
        let mut lines: Vec<String> = Vec::new();
        while let Ok(Some(line)) = reader.next_line().await {
            let line = strip_ansi(&line);
            tracing::debug!(target: "yt-dlp", "{line}");
            lines.push(line);
            if lines.len() > 200 {
                lines.drain(..100);
            }
        }
        lines
    });

    let mut reader = LineReader::new(stdout);
    let mut tracker = ProgressTracker::default();
    let mut file: Option<FileInfo> = None;
    let mut last = Progress::default();
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut already_downloaded = false;
    let mut seen_files: Vec<PathBuf> = Vec::new();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                kill_tree(&mut child).await;
                return Err(JobError::Cancelled);
            }
            line = reader.next_line() => {
                let Ok(Some(line)) = line else { break };
                let line = strip_ansi(&line);
                match parse_line(&line) {
                    OutLine::Progress(p) => {
                        for f in &p.files {
                            if !seen_files.contains(f) {
                                seen_files.push(f.clone());
                                on_file(f.clone());
                            }
                        }
                        let finished = p.status == "finished";
                        last = tracker.update(&p);
                        if finished || last_emit.elapsed() >= Duration::from_millis(250) {
                            on_progress(last.clone());
                            last_emit = Instant::now();
                        }
                        if let Some(g) = guard.as_mut() {
                            space_error = g.poll().err();
                        }
                    }
                    OutLine::PostProcess { status, name } => {
                        if let Some(g) = guard.as_mut() {
                            space_error = g.poll().err();
                        }
                        if status == "started" || status == "processing" {
                            let mut p = last.clone();
                            p.stage = Some(postprocessor_label(&name).to_string());
                            p.speed = None;
                            p.eta = None;
                            on_progress(p);
                        }
                    }
                    OutLine::ExpectedSize(size) => {
                        tracker.set_expected_total(size);
                        if let (Some(g), Some(total)) = (guard.as_ref(), size) {
                            // Parts already on disk (resume) don't count again; merging
                            // video+audio briefly needs a second copy.
                            let existing = crate::disk::dir_size(&job.work_dir());
                            let peak = match job.options.media {
                                MediaKind::Video => total,
                                MediaKind::Audio => total / 2,
                            };
                            space_error = g.check_needed(total.saturating_sub(existing) + peak).err();
                        }
                    }
                    OutLine::File(info) => file = Some(info),
                    OutLine::Other(text) => {
                        if text.contains("has already been recorded in the archive")
                            || text.contains("has already been downloaded")
                        {
                            already_downloaded = true;
                        }
                        tracing::debug!(target: "yt-dlp", "{text}");
                    }
                }
                if let Some(msg) = space_error.take() {
                    // Partial files stay, so Retry resumes once space is freed.
                    kill_tree(&mut child).await;
                    return Err(JobError::Failed(msg));
                }
            }
        }
    }

    let status = tokio::select! {
        _ = cancel.cancelled() => {
            kill_tree(&mut child).await;
            return Err(JobError::Cancelled);
        }
        s = child.wait() => s?,
    };
    let stderr_lines = stderr_task.await.unwrap_or_default();
    drop(info_file);

    if !status.success() {
        return Err(JobError::Failed(explain_error(&stderr_lines, job.site, auth.signed_out.as_deref())));
    }
    if stderr_lines.iter().any(|l| l.contains("has already been recorded in the archive")) {
        already_downloaded = true;
    }
    // Move the result (and any side files such as unembedded subtitles) out of the
    // private working folder, never overwriting an existing file.
    let final_path = match &file {
        Some(f) => Some(
            move_out(&f.path, &job.output_dir)
                .map_err(|e| JobError::Failed(format!("could not move the finished file: {e}")))?,
        ),
        None => None,
    };
    finish_work_dir(&job.work_dir(), &job.output_dir);

    // Danmaku / comments as an .ass file next to the video (never fails the download).
    let mut comments_note = None;
    let wants_comments = job.options.comments
        && job.options.media == MediaKind::Video
        && matches!(job.site, Site::Bilibili | Site::Niconico);
    if let (true, Some(video)) = (wants_comments, &final_path) {
        on_progress(Progress {
            fraction: Some(1.0),
            stage: Some("Saving comments…".into()),
            ..Default::default()
        });
        comments_note = Some(match save_comments(ctx, job, &auth, video).await {
            Ok(0) => "No comments".to_string(),
            Ok(n) => format!("{n} comments saved"),
            Err(e) => format!("Comments not saved: {e:#}"),
        });
    }
    let signed_out_note = auth.signed_out.as_ref().map(|r| format!("Downloaded without signing in: {r}"));
    drop(auth);

    let size = match &final_path {
        Some(p) => tokio::fs::metadata(p).await.ok().map(|m| m.len()),
        None => None,
    };
    Ok(JobOutcome {
        size,
        ext: file.as_ref().and_then(|f| f.ext.clone()),
        height: file.as_ref().and_then(|f| f.height),
        fps: file.as_ref().and_then(|f| f.fps),
        note: match (&file, already_downloaded) {
            (None, true) => Some("Already downloaded earlier (skipped)".into()),
            _ => [signed_out_note, comments_note].into_iter().flatten().reduce(|a, b| format!("{a} · {b}")),
        },
        file: final_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn job(options: DownloadOptions) -> DownloadJob {
        DownloadJob {
            url: "https://www.youtube.com/watch?v=x".into(),
            site: Site::YouTube,
            options,
            output_dir: PathBuf::from("/tmp/out"),
            file_prefix: Some("03. ".into()),
            work_key: "7-1".into(),
        }
    }

    fn has_pair(args: &[String], flag: &str, value: &str) -> bool {
        args.windows(2).any(|w| w[0] == flag && w[1] == value)
    }

    #[test]
    fn video_args() {
        let s = Settings::default();
        let a = download_args(&s, &job(DownloadOptions { quality: Quality::Height(720), ..Default::default() }));
        assert!(has_pair(&a, "-f", "bv*+ba/b"));
        assert!(has_pair(&a, "-S", "res:720,vcodec:h264,acodec:aac"));
        assert!(has_pair(&a, "--merge-output-format", "mp4"));
        assert!(has_pair(&a, "-o", "03. %(title).200B.%(ext)s"));
        assert!(has_pair(&a, "-P", &PathBuf::from("/tmp/out").join(".odl-partial-7-1").to_string_lossy()));
        assert!(a.contains(&"--embed-thumbnail".to_string()));
        assert!(!a.contains(&"--write-subs".to_string()));
    }

    #[test]
    fn webm_and_subtitles() {
        let s = Settings::default();
        let a = download_args(
            &s,
            &job(DownloadOptions {
                container: VideoContainer::Webm,
                quality: Quality::Best,
                subtitles: "en,ja".into(),
                ..Default::default()
            }),
        );
        assert!(has_pair(&a, "-S", "vcodec:vp9,acodec:opus"));
        assert!(has_pair(&a, "--merge-output-format", "webm/mkv"));
        assert!(has_pair(&a, "--sub-langs", "en,ja"));
        assert!(a.contains(&"--embed-subs".to_string()));
        assert!(!a.contains(&"--embed-thumbnail".to_string()));
    }

    #[test]
    fn audio_args() {
        let s = Settings { rate_limit_kib: 2048, ..Default::default() };
        let a = download_args(
            &s,
            &job(DownloadOptions {
                media: MediaKind::Audio,
                audio_format: AudioFormat::Ogg,
                audio_bitrate: AudioBitrate::Kbps(192),
                ..Default::default()
            }),
        );
        assert!(has_pair(&a, "--audio-format", "vorbis"));
        assert!(has_pair(&a, "--audio-quality", "192K"));
        assert!(has_pair(&a, "-r", "2048K"));
        assert!(a.contains(&"-x".to_string()));
        let flac = download_args(
            &s,
            &job(DownloadOptions {
                media: MediaKind::Audio,
                audio_format: AudioFormat::Flac,
                audio_bitrate: AudioBitrate::Kbps(128),
                ..Default::default()
            }),
        );
        assert!(has_pair(&flac, "--audio-quality", "0"));
    }

    #[test]
    fn prefix_percent_is_escaped() {
        let mut j = job(DownloadOptions::default());
        j.file_prefix = Some("100% ".into());
        let a = download_args(&Settings::default(), &j);
        assert!(has_pair(&a, "-o", "100%% %(title).200B.%(ext)s"));
    }

    #[test]
    fn split_extra_args() {
        assert_eq!(split_args(r#"--foo "a b" 'c d' e"#), vec!["--foo", "a b", "c d", "e"]);
        assert_eq!(split_args(r#"--x """#), vec!["--x", ""]);
        assert!(split_args("   ").is_empty());
    }

    #[test]
    fn parses_progress_lines() {
        let l = parse_line("[odl] downloading|1048576|NA|10485760|524288.5|17|NA|NA|137");
        let OutLine::Progress(p) = l else { panic!("{l:?}") };
        assert_eq!(p.downloaded, Some(1_048_576));
        assert_eq!(p.total, None);
        assert_eq!(p.total_estimate, Some(10_485_760));
        assert_eq!(p.speed, Some(524_288.5));
        assert_eq!(p.eta, Some(17));
        assert_eq!(p.format_id, "137");
        assert!(p.files.is_empty());

        let l = parse_line("[odl] downloading|1|2|NA|NA|NA|NA|NA|30080|/d/A | B.f30080.mp4.part\u{1f}/d/A | B.f30080.mp4");
        let OutLine::Progress(p) = l else { panic!("{l:?}") };
        assert_eq!(p.format_id, "30080");
        assert_eq!(p.files, vec![PathBuf::from("/d/A | B.f30080.mp4.part"), PathBuf::from("/d/A | B.f30080.mp4")]);

        assert_eq!(
            parse_line("[odlpp] started|Merger"),
            OutLine::PostProcess { status: "started".into(), name: "Merger".into() }
        );
        assert_eq!(parse_line("[odlsize] 1234.0"), OutLine::ExpectedSize(Some(1234)));
        assert_eq!(parse_line("[odlsize] NA"), OutLine::ExpectedSize(None));
        let OutLine::File(f) = parse_line("[odlfile] 1080|29.97|mp4|/home/u/My | Video.mp4") else { panic!() };
        assert_eq!(f.height, Some(1080));
        assert_eq!(f.fps, Some(29.97));
        assert_eq!(f.path, PathBuf::from("/home/u/My | Video.mp4"));
        assert!(matches!(parse_line("[youtube] x: Downloading webpage"), OutLine::Other(_)));
    }

    #[test]
    fn tracker_combines_streams() {
        let mut t = ProgressTracker::default();
        t.set_expected_total(Some(110));
        let mk = |status: &str, d: u64, total: u64, fid: &str| RawProgress {
            status: status.into(),
            downloaded: Some(d),
            total: Some(total),
            total_estimate: None,
            speed: None,
            eta: None,
            fragment_index: None,
            fragment_count: None,
            format_id: fid.into(),
            files: vec![],
        };
        let p = t.update(&mk("downloading", 50, 100, "137"));
        assert_eq!((p.downloaded, p.total), (50, Some(110)));
        let p = t.update(&mk("finished", 100, 100, "137"));
        assert_eq!(p.downloaded, 100);
        let p = t.update(&mk("downloading", 5, 10, "140"));
        assert_eq!((p.downloaded, p.total), (105, Some(110)));
        let p = t.update(&mk("finished", 10, 10, "140"));
        assert_eq!(p.fraction, Some(1.0));
    }

    #[test]
    fn parses_single_video_probe() {
        let v = json!({
            "id": "abc", "title": "Song", "uploader": "Artist", "duration": 312.0,
            "webpage_url": "https://www.youtube.com/watch?v=abc",
            "thumbnails": [
                {"url": "https://i.ytimg.com/vi/abc/default.jpg", "width": 120},
                {"url": "https://i.ytimg.com/vi/abc/mqdefault.jpg", "width": 320},
                {"url": "https://i.ytimg.com/vi/abc/maxres.jpg", "width": 1280}
            ],
            "formats": [
                {"format_id": "140", "vcodec": "none", "acodec": "mp4a.40.2", "abr": 129.0, "filesize": 5000},
                {"format_id": "251", "vcodec": "none", "acodec": "opus", "abr": 135.0, "filesize": 5200},
                {"format_id": "136", "vcodec": "avc1", "acodec": "none", "height": 720, "tbr": 1500.0, "filesize": 30000},
                {"format_id": "137", "vcodec": "avc1", "acodec": "none", "height": 1080, "tbr": 3000.0, "filesize_approx": 60000},
                {"format_id": "sb0", "vcodec": "none", "acodec": "none", "height": 90}
            ],
            "subtitles": {"en": [], "ja": [], "live_chat": []}
        });
        let ProbeResult::Single(m) = parse_probe(&v).unwrap() else { panic!() };
        assert_eq!(m.heights, vec![720, 1080]);
        assert_eq!(m.video_sizes, vec![(720, 30000), (1080, 60000)]);
        assert_eq!(m.audio_size, Some(5200));
        assert_eq!(m.subtitle_langs, vec!["en", "ja"]);
        assert_eq!(m.thumbnail.as_deref(), Some("https://i.ytimg.com/vi/abc/mqdefault.jpg"));
    }

    #[test]
    fn parses_flat_playlist_probe() {
        let v = json!({
            "_type": "playlist", "id": "PL1", "title": "MOIRA - カラオケ", "uploader": "someone",
            "entries": [
                {"_type": "url", "ie_key": "Youtube", "id": "a1", "url": "https://www.youtube.com/watch?v=a1",
                 "title": "First", "duration": 251.0,
                 "thumbnails": [{"url": "https://i.ytimg.com/vi/a1/hqdefault.jpg", "width": 336}]},
                null,
                {"_type": "url", "ie_key": "Youtube", "id": "a2", "url": "a2", "title": "Second"},
                {"_type": "url", "ie_key": "Generic", "id": "zz", "url": "zz"}
            ]
        });
        let ProbeResult::Playlist(p) = parse_probe(&v).unwrap() else { panic!() };
        assert_eq!(p.title, "MOIRA - カラオケ");
        assert_eq!(p.entries.len(), 2);
        assert_eq!(p.entries[1].url, "https://www.youtube.com/watch?v=a2");
        assert_eq!(p.entries[1].index, 2);
        assert_eq!(p.entries[0].duration, Some(251.0));
    }

    #[test]
    fn deletes_partial_files_only() {
        let dir = std::env::temp_dir().join(format!("odl-partial-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let part = dir.join("Video.f137.mp4.part");
        for name in ["Video.f137.mp4.part", "Video.f137.mp4.part.ytdl", "Video.f137.mp4.part-Frag3", "Other.mp4"] {
            std::fs::write(dir.join(name), b"1234").unwrap();
        }
        let (n, bytes) = delete_partial_files(&[part, dir.join("Video.f137.mp4")]);
        assert_eq!((n, bytes), (3, 12));
        assert!(dir.join("Other.mp4").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn finished_files_never_overwrite() {
        let base = std::env::temp_dir().join(format!("odl-move-test-{}", std::process::id()));
        let work = base.join(".odl-partial-1-1");
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(base.join("Song.mp4"), b"video").unwrap();
        std::fs::write(work.join("Song.mp4"), b"audio").unwrap();
        std::fs::write(work.join("Song.en.vtt"), b"subs").unwrap();
        std::fs::write(work.join("Song.f1.mp4.part"), b"x").unwrap();
        let moved = move_out(&work.join("Song.mp4"), &base).unwrap();
        assert_eq!(moved, base.join("Song (1).mp4"));
        assert_eq!(std::fs::read(base.join("Song.mp4")).unwrap(), b"video");
        finish_work_dir(&work, &base);
        assert!(base.join("Song.en.vtt").exists());
        assert!(!work.exists());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn friendly_errors() {
        let lines = vec![
            "WARNING: something".to_string(),
            "ERROR: [youtube] abc: Sign in to confirm you're not a bot".to_string(),
        ];
        let msg = friendly_error(&lines, Site::YouTube);
        assert!(msg.starts_with("[youtube] abc: Sign in"));
        assert!(msg.contains("Settings → Accounts"));
    }

    #[test]
    fn explains_skipped_sign_in_only_when_an_account_is_needed() {
        let members = vec!["ERROR: [youtube] abc: Join this channel to get access to members-only content".to_string()];
        let msg = explain_error(&members, Site::YouTube, Some("Vivaldi is open and locks its cookies"));
        assert!(msg.contains("needs a signed-in account, but Vivaldi is open"), "{msg}");
        assert!(friendly_error(&members, Site::YouTube).contains("Tip: sign in to YouTube"));
        let forbidden = vec!["ERROR: unable to download video data: HTTP Error 403: Forbidden".to_string()];
        assert!(!explain_error(&forbidden, Site::YouTube, Some("x")).contains("signed-in account"));
    }

    #[test]
    fn cookie_read_errors_get_their_own_tips() {
        let locked = vec!["ERROR: Could not copy Chrome cookie database. See  https://github.com/yt-dlp/yt-dlp/issues/7271  for more info".to_string()];
        let msg = friendly_error(&locked, Site::YouTube);
        assert!(msg.contains("cookies.txt") && !msg.contains("disable-features") && !msg.contains("Tip: sign in"), "{msg}");
        let dpapi = vec!["ERROR: Failed to decrypt with DPAPI. See https://github.com/yt-dlp/yt-dlp/issues/10927".to_string()];
        assert!(friendly_error(&dpapi, Site::YouTube).contains("Use Firefox or a cookies.txt file"));
        let rotated = vec![
            "WARNING: [youtube] The provided YouTube account cookies are no longer valid. They have likely been rotated in the browser as a security measure.".to_string(),
            "ERROR: [youtube] abc: Sign in to confirm you're not a bot".to_string(),
        ];
        assert!(friendly_error(&rotated, Site::YouTube).contains("Export them again"));
    }

    #[test]
    fn youtube_client_list() {
        let plain = youtube_args(None);
        assert_eq!(plain, vec!["--extractor-args", "youtube:player_client=default,web_embedded"]);

        let pot = crate::pot::PotProvider {
            plugins_dir: PathBuf::from("/d/bgutil/plugins"),
            server_dir: PathBuf::from("/d/bgutil/server"),
        };
        let with_pot = youtube_args(Some(&pot));
        assert!(has_pair(&with_pot, "--plugin-dirs", "/d/bgutil/plugins"));
        assert!(has_pair(&with_pot, "--extractor-args", "youtube:player_client=default,web_embedded,mweb"));
    }

    #[test]
    fn ytdlp_from_path_may_fetch_challenge_scripts() {
        let ctx_with = |source| RunCtx {
            settings: Settings::default(),
            tools: crate::tools::ToolPaths {
                ytdlp: Some(crate::tools::ResolvedTool { path: PathBuf::from("yt-dlp"), source }),
                ..Default::default()
            },
        };
        let system = site_args(&ctx_with(ToolSource::System), Site::YouTube);
        assert!(has_pair(&system, "--remote-components", "ejs:github"));
        let managed = site_args(&ctx_with(ToolSource::Managed), Site::YouTube);
        assert!(!managed.contains(&"--remote-components".to_string()));
        assert!(site_args(&ctx_with(ToolSource::System), Site::Niconico).is_empty());
    }
}
