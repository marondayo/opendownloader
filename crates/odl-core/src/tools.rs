//! External components: yt-dlp (extraction), ffmpeg (muxing/conversion) and deno
//! (JavaScript runtime yt-dlp uses for YouTube).
//!
//! Resolution order for each tool: explicit path from settings → copy managed by
//! OpenDownloader in its data dir → `PATH`.

use crate::process::{command, find_in_path, is_executable};
use crate::settings::Settings;
use anyhow::{anyhow, bail, Context};
use futures_util::StreamExt;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    YtDlp,
    Ffmpeg,
    Deno,
}

impl Tool {
    pub const ALL: [Tool; 3] = [Tool::YtDlp, Tool::Ffmpeg, Tool::Deno];

    pub fn label(self) -> &'static str {
        match self {
            Tool::YtDlp => "yt-dlp",
            Tool::Ffmpeg => "FFmpeg",
            Tool::Deno => "Deno",
        }
    }

    pub fn purpose(self) -> &'static str {
        match self {
            Tool::YtDlp => "Extracts and downloads media from every supported site (required).",
            Tool::Ffmpeg => "Merges video and audio, converts audio and embeds metadata (required).",
            Tool::Deno => "JavaScript runtime yt-dlp needs to unlock all YouTube formats (recommended).",
        }
    }

    fn base_name(self) -> &'static str {
        match self {
            Tool::YtDlp => "yt-dlp",
            Tool::Ffmpeg => "ffmpeg",
            Tool::Deno => "deno",
        }
    }

    pub fn exe_name(self) -> String {
        if cfg!(windows) {
            format!("{}.exe", self.base_name())
        } else {
            self.base_name().to_string()
        }
    }

    /// Location of the copy managed by OpenDownloader.
    pub fn managed_path(self) -> PathBuf {
        crate::paths::get().bin_dir().join(self.exe_name())
    }

    fn configured(self, settings: &Settings) -> Option<&PathBuf> {
        match self {
            Tool::YtDlp => settings.ytdlp_path.as_ref(),
            Tool::Ffmpeg => settings.ffmpeg_path.as_ref(),
            Tool::Deno => settings.deno_path.as_ref(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSource {
    Custom,
    Managed,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTool {
    pub path: PathBuf,
    pub source: ToolSource,
}

#[derive(Debug, Clone, Default)]
pub struct ToolPaths {
    pub ytdlp: Option<ResolvedTool>,
    pub ffmpeg: Option<ResolvedTool>,
    pub deno: Option<ResolvedTool>,
}

impl ToolPaths {
    pub fn get(&self, tool: Tool) -> Option<&ResolvedTool> {
        match tool {
            Tool::YtDlp => self.ytdlp.as_ref(),
            Tool::Ffmpeg => self.ffmpeg.as_ref(),
            Tool::Deno => self.deno.as_ref(),
        }
    }
}

pub fn resolve_one(tool: Tool, settings: &Settings) -> Option<ResolvedTool> {
    if let Some(p) = tool.configured(settings).filter(|p| !p.as_os_str().is_empty()) {
        return Some(ResolvedTool { path: p.clone(), source: ToolSource::Custom });
    }
    let managed = tool.managed_path();
    if is_executable(&managed) {
        return Some(ResolvedTool { path: managed, source: ToolSource::Managed });
    }
    find_in_path(tool.base_name()).map(|path| ResolvedTool { path, source: ToolSource::System })
}

pub fn resolve(settings: &Settings) -> ToolPaths {
    ToolPaths {
        ytdlp: resolve_one(Tool::YtDlp, settings),
        ffmpeg: resolve_one(Tool::Ffmpeg, settings),
        deno: resolve_one(Tool::Deno, settings),
    }
}

/// Runs the tool and returns its version string.
pub async fn version(tool: Tool, path: &Path) -> anyhow::Result<String> {
    let arg = match tool {
        Tool::YtDlp => "--version",
        Tool::Ffmpeg => "-version",
        Tool::Deno => "--version",
    };
    let out = tokio::time::timeout(std::time::Duration::from_secs(20), command(path).arg(arg).output())
        .await
        .map_err(|_| anyhow!("{} did not respond", tool.label()))??;
    if !out.status.success() {
        bail!("{} exited with {}", tool.label(), out.status);
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let first = text.lines().next().unwrap_or("").trim();
    Ok(match tool {
        Tool::YtDlp => first.to_string(),
        // "ffmpeg version 7.1-static Copyright ..." / "deno 2.1.4 (stable, ...)"
        Tool::Ffmpeg => first.split_whitespace().nth(2).unwrap_or(first).to_string(),
        Tool::Deno => first.split_whitespace().nth(1).unwrap_or(first).to_string(),
    })
}

pub(crate) fn http() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("OpenDownloader/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(20))
        .build()?)
}

/// Latest yt-dlp release tag (e.g. `2026.09.30`).
pub async fn latest_ytdlp_version() -> anyhow::Result<String> {
    #[derive(serde::Deserialize)]
    struct Release {
        tag_name: String,
    }
    let r: Release = http()?
        .get("https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(r.tag_name)
}

enum Archive {
    None,
    Zip,
    TarXz,
}

fn asset(tool: Tool) -> anyhow::Result<(String, Archive)> {
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    let unsupported = || anyhow!("no prebuilt {} for {os}/{arch}; install it with your package manager", tool.label());
    Ok(match tool {
        Tool::YtDlp => {
            let name = match (os, arch) {
                ("linux", "x86_64") => "yt-dlp_linux",
                ("linux", "aarch64") => "yt-dlp_linux_aarch64",
                ("windows", "x86_64") => "yt-dlp.exe",
                ("windows", "aarch64") => "yt-dlp_arm64.exe",
                ("macos", _) => "yt-dlp_macos",
                _ => return Err(unsupported()),
            };
            (format!("https://github.com/yt-dlp/yt-dlp/releases/latest/download/{name}"), Archive::None)
        }
        Tool::Deno => {
            let target = match (os, arch) {
                ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
                ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
                ("windows", "x86_64") => "x86_64-pc-windows-msvc",
                ("macos", "x86_64") => "x86_64-apple-darwin",
                ("macos", "aarch64") => "aarch64-apple-darwin",
                _ => return Err(unsupported()),
            };
            (format!("https://github.com/denoland/deno/releases/latest/download/deno-{target}.zip"), Archive::Zip)
        }
        Tool::Ffmpeg => {
            let (name, kind) = match (os, arch) {
                ("linux", "x86_64") => ("ffmpeg-master-latest-linux64-gpl.tar.xz", Archive::TarXz),
                ("linux", "aarch64") => ("ffmpeg-master-latest-linuxarm64-gpl.tar.xz", Archive::TarXz),
                ("windows", "x86_64") => ("ffmpeg-master-latest-win64-gpl.zip", Archive::Zip),
                ("windows", "aarch64") => ("ffmpeg-master-latest-winarm64-gpl.zip", Archive::Zip),
                _ => return Err(unsupported()),
            };
            (format!("https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/{name}"), kind)
        }
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum InstallProgress {
    Downloading { downloaded: u64, total: Option<u64> },
    Extracting,
    /// Installing dependencies (PO-token provider).
    Installing,
}

pub(crate) async fn download_file(
    url: &str,
    dest: &Path,
    progress: &(dyn Fn(InstallProgress) + Send + Sync),
) -> anyhow::Result<()> {
    let resp = http()?.get(url).send().await?.error_for_status()?;
    let total = resp.content_length();
    let mut file = tokio::fs::File::create(dest).await.with_context(|| format!("creating {}", dest.display()))?;
    let mut stream = resp.bytes_stream();
    let mut downloaded = 0u64;
    let mut last_report = std::time::Instant::now();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        if last_report.elapsed().as_millis() > 150 {
            progress(InstallProgress::Downloading { downloaded, total });
            last_report = std::time::Instant::now();
        }
    }
    file.flush().await?;
    progress(InstallProgress::Downloading { downloaded, total });
    Ok(())
}

fn make_executable(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Copies the entries whose file name is in `wanted` from a zip archive into `dest_dir`.
fn extract_zip(archive: &Path, wanted: &[String], dest_dir: &Path) -> anyhow::Result<usize> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
    let mut found = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(name) = entry.enclosed_name().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        else {
            continue;
        };
        if entry.is_file() && wanted.contains(&name) {
            let out_path = dest_dir.join(&name);
            let tmp = out_path.with_extension("download");
            let mut out = std::fs::File::create(&tmp)?;
            std::io::copy(&mut entry, &mut out)?;
            drop(out);
            make_executable(&tmp)?;
            std::fs::rename(&tmp, &out_path)?;
            found += 1;
        }
    }
    Ok(found)
}

fn extract_tar_xz(archive: &Path, wanted: &[String], dest_dir: &Path) -> anyhow::Result<usize> {
    let tar_path = archive.with_extension("tar");
    {
        let mut input = std::io::BufReader::new(std::fs::File::open(archive)?);
        let mut output = std::io::BufWriter::new(std::fs::File::create(&tar_path)?);
        lzma_rs::xz_decompress(&mut input, &mut output).map_err(|e| anyhow!("xz: {e:?}"))?;
    }
    let mut tar = tar::Archive::new(std::fs::File::open(&tar_path)?);
    let mut found = 0;
    for entry in tar.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        if entry.header().entry_type().is_file() && wanted.contains(&name) {
            let out_path = dest_dir.join(&name);
            let tmp = out_path.with_extension("download");
            let mut out = std::fs::File::create(&tmp)?;
            std::io::copy(&mut entry, &mut out)?;
            drop(out);
            make_executable(&tmp)?;
            std::fs::rename(&tmp, &out_path)?;
            found += 1;
        }
    }
    let _ = std::fs::remove_file(&tar_path);
    Ok(found)
}

/// Downloads the latest build of a tool into OpenDownloader's managed bin dir.
pub async fn install(tool: Tool, progress: impl Fn(InstallProgress) + Send + Sync + 'static) -> anyhow::Result<PathBuf> {
    let (url, kind) = asset(tool)?;
    let bin = crate::paths::get().bin_dir();
    tokio::fs::create_dir_all(&bin).await?;
    // Archive + unpacked copy at the same time (FFmpeg's tar.xz unpacks to ~600 MB).
    let needed: u64 = match tool {
        Tool::Ffmpeg => 900 << 20,
        Tool::Deno => 300 << 20,
        Tool::YtDlp => 100 << 20,
    };
    if let Some(free) = crate::disk::free_space(&bin) {
        if free < needed {
            bail!(
                "not enough disk space to install {}: needs about {}, {} free",
                tool.label(),
                crate::model::format_bytes(needed),
                crate::model::format_bytes(free)
            );
        }
    }
    let target = tool.managed_path();
    let suffix = match kind {
        Archive::None => "download",
        Archive::Zip => "zip",
        Archive::TarXz => "tar.xz",
    };
    let tmp = bin.join(format!(".{}.{suffix}", tool.base_name()));
    download_file(&url, &tmp, &progress).await.with_context(|| format!("downloading {url}"))?;

    match kind {
        Archive::None => {
            make_executable(&tmp)?;
            tokio::fs::rename(&tmp, &target).await?;
        }
        Archive::Zip | Archive::TarXz => {
            progress(InstallProgress::Extracting);
            let mut wanted = vec![tool.exe_name()];
            if tool == Tool::Ffmpeg {
                wanted.push(if cfg!(windows) { "ffprobe.exe".into() } else { "ffprobe".into() });
            }
            let tmp2 = tmp.clone();
            let bin2 = bin.clone();
            let found = tokio::task::spawn_blocking(move || match kind {
                Archive::Zip => extract_zip(&tmp2, &wanted, &bin2),
                _ => extract_tar_xz(&tmp2, &wanted, &bin2),
            })
            .await??;
            let _ = tokio::fs::remove_file(&tmp).await;
            if found == 0 {
                bail!("{} archive did not contain the expected binary", tool.label());
            }
        }
    }
    version(tool, &target).await.context("the downloaded binary does not run")?;
    Ok(target)
}

/// Updates yt-dlp: re-downloads the managed copy, or runs `yt-dlp -U` for other installs.
pub async fn update_ytdlp(
    resolved: &ResolvedTool,
    progress: impl Fn(InstallProgress) + Send + Sync + 'static,
) -> anyhow::Result<String> {
    if resolved.source == ToolSource::Managed {
        let path = install(Tool::YtDlp, progress).await?;
        return version(Tool::YtDlp, &path).await;
    }
    let out = command(&resolved.path).arg("-U").output().await?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if !out.status.success() || text.contains("ERROR") {
        bail!(
            "{}\nTip: install the managed copy from Settings → Components to get automatic updates.",
            text.trim()
        );
    }
    version(Tool::YtDlp, &resolved.path).await
}

/// Whether `latest` (a yt-dlp tag like `2026.09.30` or `2026.09.30.1`) is newer than `current`.
pub fn is_newer_version(current: &str, latest: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> { s.trim().split('.').filter_map(|p| p.parse().ok()).collect() };
    parse(latest) > parse(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(is_newer_version("2025.01.15", "2025.02.01"));
        assert!(is_newer_version("2025.01.15", "2025.01.15.1"));
        assert!(!is_newer_version("2025.01.15", "2025.01.15"));
        assert!(!is_newer_version("2025.10.01", "2025.09.30"));
    }

    #[test]
    fn assets_exist_for_linux_x64() {
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            for t in Tool::ALL {
                assert!(asset(t).is_ok());
            }
        }
    }
}
