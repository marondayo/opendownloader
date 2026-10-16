//! OpenDownloader's own updates: finding new releases on GitHub and installing them.
//!
//! How an update is applied depends on how OpenDownloader was installed:
//! - AppImage: the AppImage file is replaced.
//! - Standalone binary (Linux tarball / `install.sh`, Windows portable zip): the executable is
//!   swapped in place. The running program keeps its old copy until it exits.
//! - Windows installer: the new setup runs once OpenDownloader has exited.
//! - Package manager (.deb) or a development build: only the release page is offered.

use crate::tools::{download_file, http, make_executable, InstallProgress};
use anyhow::{anyhow, bail, Context};
use std::path::{Path, PathBuf};

/// Version of this build.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

/// Page listing all releases.
pub fn releases_url() -> String {
    format!("{REPOSITORY}/releases")
}

/// How this copy of OpenDownloader was installed, which decides how it can update itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Install {
    /// Linux AppImage at this path.
    AppImage(PathBuf),
    /// A standalone executable we can replace (Linux tarball, Windows portable zip).
    Binary(PathBuf),
    /// Installed with the Windows installer (Inno Setup).
    WindowsSetup,
    /// Can't update itself; the reason is shown to the user.
    Manual(String),
}

impl Install {
    pub fn detect() -> Install {
        if let Some(appimage) = std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file()) {
            let dir = appimage.parent().unwrap_or(Path::new("."));
            if !dir_writable(dir) {
                return Install::Manual(format!("no write access to {}", dir.display()));
            }
            return Install::AppImage(appimage);
        }
        if cfg!(debug_assertions) {
            return Install::Manual("this is a development build".into());
        }
        let Ok(exe) = std::env::current_exe() else {
            return Install::Manual("the program's location is unknown".into());
        };
        // Resolve symlinks such as ~/.local/bin/opendownloader → the real file.
        #[cfg(unix)]
        let exe = exe.canonicalize().unwrap_or(exe);
        let Some(dir) = exe.parent() else {
            return Install::Manual("the program's location is unknown".into());
        };
        // Cargo's target/<profile> dir.
        if dir.join(".fingerprint").is_dir() {
            return Install::Manual("this is a development build".into());
        }
        // Updates never ask for administrator rights: only what the user can write is replaced.
        let writable = dir_writable(dir);
        if cfg!(windows) && has_uninstaller(dir) {
            // Per-user installs (the default) update silently. Setup reuses the previous install
            // mode, so updating an all-users install in Program Files would prompt for elevation.
            return if writable {
                Install::WindowsSetup
            } else {
                Install::Manual("it was installed for all users, which needs administrator rights to update".into())
            };
        }
        if cfg!(unix) && exe.starts_with("/usr") && !exe.starts_with("/usr/local") {
            return Install::Manual("it was installed with your system's package manager".into());
        }
        if !writable {
            return Install::Manual(format!("no write access to {}", dir.display()));
        }
        Install::Binary(exe)
    }

    pub fn can_self_update(&self) -> bool {
        !matches!(self, Install::Manual(_))
    }
}

fn has_uninstaller(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|e| {
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            name.starts_with("unins") && name.ends_with(".exe")
        })
    })
}

fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".opendownloader-write-test-{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// A published release newer than this build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Version without the leading `v` (e.g. `0.2.0`).
    pub version: String,
    /// Release notes (Markdown).
    pub notes: String,
    /// Release page on GitHub.
    pub page: String,
    /// Marked as a pre-release (beta) on GitHub.
    pub prerelease: bool,
    /// The package for this kind of install, if the release has one.
    pub asset: Option<Asset>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// Hex SHA-256 published by GitHub.
    pub sha256: Option<String>,
}

impl Release {
    /// Whether OpenDownloader can download and install this release by itself.
    pub fn installable(&self, install: &Install) -> bool {
        install.can_self_update() && self.asset.is_some()
    }
}

/// End of the release asset name for an install kind on `os`/`arch` (see `.github/workflows/release.yml`).
fn asset_suffix(install: &Install, os: &str, arch: &str) -> Option<String> {
    match (install, os, arch) {
        (Install::AppImage(_), "linux", _) => Some(format!("-{arch}.AppImage")),
        (Install::Binary(_), "linux", _) => Some(format!("-linux-{arch}.tar.gz")),
        (Install::Binary(_), "windows", "x86_64") => Some("-windows-x64-portable.zip".into()),
        (Install::WindowsSetup, "windows", "x86_64") => Some("-windows-x64-setup.exe".into()),
        _ => None,
    }
}

#[derive(Debug, serde::Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Debug, serde::Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

/// The highest published release newer than `current`; pre-releases only when asked for.
fn pick_newest(releases: Vec<GhRelease>, current: &str, include_prereleases: bool) -> Option<GhRelease> {
    let version = |r: &GhRelease| r.tag_name.trim().trim_start_matches('v').to_string();
    releases
        .into_iter()
        .filter(|r| !r.draft && (include_prereleases || !r.prerelease) && is_newer(current, &version(r)))
        .fold(None, |best: Option<GhRelease>, r| match best {
            Some(b) if !is_newer(&version(&b), &version(&r)) => Some(b),
            _ => Some(r),
        })
}

/// Looks up the newest release; `None` when this build is up to date.
pub async fn check(install: &Install, include_prereleases: bool) -> anyhow::Result<Option<Release>> {
    let api = format!(
        "{}/releases?per_page=30",
        REPOSITORY.replacen("https://github.com/", "https://api.github.com/repos/", 1)
    );
    let releases: Vec<GhRelease> = http()?
        .get(&api)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let Some(r) = pick_newest(releases, CURRENT_VERSION, include_prereleases) else {
        return Ok(None);
    };
    let version = r.tag_name.trim().trim_start_matches('v').to_string();
    let suffix = asset_suffix(install, std::env::consts::OS, std::env::consts::ARCH);
    let asset = suffix.and_then(|s| r.assets.into_iter().find(|a| a.name.ends_with(&s))).map(|a| Asset {
        sha256: a.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")).map(str::to_ascii_lowercase),
        name: a.name,
        url: a.browser_download_url,
        size: a.size,
    });
    Ok(Some(Release {
        version,
        notes: r.body.unwrap_or_default().trim().to_string(),
        page: r.html_url,
        prerelease: r.prerelease,
        asset,
    }))
}

/// Whether version `latest` is newer than `current` (`1.2.3`, optional `v` prefix and `-pre` suffix).
pub fn is_newer(current: &str, latest: &str) -> bool {
    fn key(v: &str) -> (Vec<u64>, bool, String) {
        let v = v.trim().trim_start_matches('v');
        let (core, pre) = match v.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (v, None),
        };
        let mut nums: Vec<u64> = core.split('.').map(|p| p.parse().unwrap_or(0)).collect();
        while nums.len() > 1 && nums.last() == Some(&0) {
            nums.pop();
        }
        // A release sorts after its pre-releases.
        (nums, pre.is_none(), pre.unwrap_or("").to_string())
    }
    key(latest) > key(current)
}

/// A downloaded update, waiting to be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Staged {
    /// The program file was replaced; the new version runs after a restart.
    Replaced,
    /// A downloaded installer to run once OpenDownloader has exited.
    Setup(PathBuf),
}

/// Downloads a release and installs it as far as possible while the app is running.
pub async fn download(
    install: &Install,
    release: &Release,
    progress: impl Fn(InstallProgress) + Send + Sync + 'static,
) -> anyhow::Result<Staged> {
    let asset = release.asset.as_ref().ok_or_else(|| anyhow!("this release has no package for your system"))?;
    let dir = crate::paths::get().updates_dir();
    match install {
        Install::AppImage(target) => {
            // Same folder as the target so the final rename is atomic.
            let tmp = with_suffix(target, ".download");
            fetch(asset, &tmp, &progress).await?;
            make_executable(&tmp)?;
            std::fs::rename(&tmp, target).with_context(|| format!("replacing {}", target.display()))?;
            Ok(Staged::Replaced)
        }
        Install::Binary(exe) => {
            tokio::fs::create_dir_all(&dir).await?;
            let archive = dir.join(&asset.name);
            fetch(asset, &archive, &progress).await?;
            progress(InstallProgress::Extracting);
            let (exe, new) = (exe.clone(), with_suffix(exe, ".new"));
            tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                extract_program(&archive, &new)?;
                let _ = std::fs::remove_file(&archive);
                make_executable(&new)?;
                swap_in(&exe, &new).with_context(|| format!("replacing {}", exe.display()))
            })
            .await??;
            Ok(Staged::Replaced)
        }
        Install::WindowsSetup => {
            tokio::fs::create_dir_all(&dir).await?;
            let setup = dir.join(&asset.name);
            fetch(asset, &setup, &progress).await?;
            Ok(Staged::Setup(setup))
        }
        Install::Manual(why) => bail!("OpenDownloader can't update itself: {why}"),
    }
}

/// Downloads an asset and checks its size and checksum.
async fn fetch(asset: &Asset, dest: &Path, progress: &(dyn Fn(InstallProgress) + Send + Sync)) -> anyhow::Result<()> {
    download_file(&asset.url, dest, progress).await.with_context(|| format!("downloading {}", asset.name))?;
    let path = dest.to_path_buf();
    let (size, sha256) = tokio::task::spawn_blocking(move || -> std::io::Result<(u64, String)> {
        Ok((std::fs::metadata(&path)?.len(), sha256_hex(&path)?))
    })
    .await??;
    let bad = if size != asset.size {
        Some(format!("expected {} bytes, got {size}", asset.size))
    } else {
        asset.sha256.as_ref().filter(|want| **want != sha256).map(|_| "checksum mismatch".to_string())
    };
    if let Some(why) = bad {
        let _ = tokio::fs::remove_file(dest).await;
        bail!("{} is damaged ({why}); try again later", asset.name);
    }
    Ok(())
}

fn sha256_hex(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    std::io::copy(&mut std::fs::File::open(path)?, &mut hasher)?;
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Copies the OpenDownloader executable out of a release archive (.tar.gz or .zip).
fn extract_program(archive: &Path, out: &Path) -> anyhow::Result<()> {
    let wanted = if cfg!(windows) { "opendownloader.exe" } else { "opendownloader" };
    let name_of = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned());
    let copy = |reader: &mut dyn std::io::Read| -> anyhow::Result<()> {
        let mut file = std::fs::File::create(out).with_context(|| format!("creating {}", out.display()))?;
        std::io::copy(reader, &mut file)?;
        Ok(())
    };
    if archive.extension().is_some_and(|e| e == "zip") {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            if entry.is_file() && entry.enclosed_name().as_deref().and_then(name_of).as_deref() == Some(wanted) {
                return copy(&mut entry);
            }
        }
    } else {
        let gz = flate2::read::GzDecoder::new(std::io::BufReader::new(std::fs::File::open(archive)?));
        let mut tar = tar::Archive::new(gz);
        for entry in tar.entries()? {
            let mut entry = entry?;
            let path = entry.path()?.into_owned();
            if entry.header().entry_type().is_file() && name_of(&path).as_deref() == Some(wanted) {
                return copy(&mut entry);
            }
        }
    }
    bail!("the update package does not contain {wanted}")
}

/// Puts `new` in place of the running executable `exe`.
fn swap_in(exe: &Path, new: &Path) -> std::io::Result<()> {
    if cfg!(windows) {
        // A running .exe can't be overwritten or deleted, but it can be renamed.
        let old = with_suffix(exe, ".old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(exe, &old)?;
        if let Err(e) = std::fs::rename(new, exe) {
            let _ = std::fs::rename(&old, exe);
            return Err(e);
        }
        Ok(())
    } else {
        // The running program keeps the old file open; the new one is used from the next start.
        std::fs::rename(new, exe)
    }
}

/// Removes what earlier updates left behind (downloaded packages, the replaced .exe).
pub fn cleanup(install: &Install) {
    let _ = std::fs::remove_dir_all(crate::paths::get().updates_dir());
    if let Install::Binary(exe) = install {
        let _ = std::fs::remove_file(with_suffix(exe, ".old"));
        let _ = std::fs::remove_file(with_suffix(exe, ".new"));
    }
    if let Install::AppImage(path) = install {
        let _ = std::fs::remove_file(with_suffix(path, ".download"));
    }
}

/// Starts the updated program, or the downloaded installer. Call while exiting, after
/// settings and history are saved. `relaunch`: start OpenDownloader again afterwards
/// (the installer only does so when asked).
pub fn launch(install: &Install, staged: &Staged, relaunch: bool) -> anyhow::Result<()> {
    match staged {
        Staged::Setup(setup) => {
            let mut cmd = std::process::Command::new(setup);
            cmd.args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS"]);
            if relaunch {
                // Handled in packaging/windows/opendownloader.iss.
                cmd.arg("/relaunch=1");
            }
            cmd.spawn().with_context(|| format!("starting {}", setup.display()))?;
        }
        Staged::Replaced if relaunch => {
            let program = match install {
                Install::AppImage(p) | Install::Binary(p) => p,
                _ => bail!("nothing to restart"),
            };
            std::process::Command::new(program).spawn().with_context(|| format!("starting {}", program.display()))?;
        }
        Staged::Replaced => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_order() {
        assert!(is_newer("0.1.2", "0.1.3"));
        assert!(is_newer("0.1.2", "v0.2.0"));
        assert!(is_newer("0.9.9", "1.0.0"));
        assert!(is_newer("0.1.2", "0.1.10"));
        assert!(!is_newer("0.1.2", "0.1.2"));
        assert!(!is_newer("0.1.2", "v0.1.2"));
        assert!(!is_newer("0.1.2", "0.1.2.0"));
        assert!(!is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("0.2.0-beta.1", "0.2.0"));
        assert!(!is_newer("0.2.0", "0.2.0-beta.1"));
        assert!(is_newer("0.2.0-beta.1", "0.2.0-beta.2"));
    }

    #[test]
    fn repository_is_set() {
        assert!(REPOSITORY.starts_with("https://github.com/"), "set `repository` in Cargo.toml");
    }

    #[test]
    fn newest_release() {
        let rel = |tag: &str, draft: bool, prerelease: bool| GhRelease {
            tag_name: tag.into(),
            html_url: String::new(),
            body: None,
            draft,
            prerelease,
            assets: Vec::new(),
        };
        let list = || {
            vec![
                rel("v0.3.0", true, false),
                rel("v0.2.1-beta.1", false, true),
                rel("v0.1.3", false, false),
                rel("v0.2.0", false, false),
                rel("v0.1.2", false, false),
            ]
        };
        let tag = |r: Option<GhRelease>| r.map(|r| r.tag_name);
        // Drafts never count; pre-releases only when opted in.
        assert_eq!(tag(pick_newest(list(), "0.1.2", false)).as_deref(), Some("v0.2.0"));
        assert_eq!(tag(pick_newest(list(), "0.1.2", true)).as_deref(), Some("v0.2.1-beta.1"));
        assert_eq!(tag(pick_newest(list(), "0.2.0", false)), None);
        assert_eq!(tag(pick_newest(vec![rel("v0.1.2", false, true)], "0.1.2", true)), None);
    }

    #[test]
    fn asset_names_match_the_release_workflow() {
        let v = "0.1.3";
        let names = [
            format!("OpenDownloader-{v}-x86_64.AppImage"),
            format!("opendownloader-{v}-linux-x86_64.tar.gz"),
            format!("opendownloader_{v}-1_amd64.deb"),
            format!("OpenDownloader-{v}-windows-x64-setup.exe"),
            format!("OpenDownloader-{v}-windows-x64-portable.zip"),
        ];
        let pick = |install: &Install, os: &str, arch: &str| {
            let suffix = asset_suffix(install, os, arch)?;
            names.iter().find(|n| n.ends_with(&suffix)).cloned()
        };
        let bin = Install::Binary(PathBuf::from("x"));
        assert_eq!(pick(&Install::AppImage(PathBuf::from("x")), "linux", "x86_64"), Some(names[0].clone()));
        assert_eq!(pick(&bin, "linux", "x86_64"), Some(names[1].clone()));
        assert_eq!(pick(&Install::WindowsSetup, "windows", "x86_64"), Some(names[3].clone()));
        assert_eq!(pick(&bin, "windows", "x86_64"), Some(names[4].clone()));
        // No packages built for these.
        assert_eq!(pick(&bin, "linux", "aarch64"), None);
        assert_eq!(pick(&bin, "windows", "aarch64"), None);
        assert_eq!(pick(&Install::Manual("deb".into()), "linux", "x86_64"), None);
    }

    #[test]
    fn extracts_program_from_tarball() {
        let dir = std::env::temp_dir().join(format!("odl-update-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("pkg.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(std::fs::File::create(&archive).unwrap(), flate2::Compression::fast());
            let mut tar = tar::Builder::new(gz);
            let mut add = |path: &str, data: &[u8]| {
                let mut h = tar::Header::new_gnu();
                h.set_size(data.len() as u64);
                h.set_mode(0o755);
                h.set_cksum();
                tar.append_data(&mut h, path, data).unwrap();
            };
            add("opendownloader-0.1.3-linux-x86_64/install.sh", b"#!/bin/sh");
            add("opendownloader-0.1.3-linux-x86_64/target/release/opendownloader", b"new program");
            tar.into_inner().unwrap().finish().unwrap();
        }
        let out = dir.join("out");
        if cfg!(windows) {
            assert!(extract_program(&archive, &out).is_err());
        } else {
            extract_program(&archive, &out).unwrap();
            assert_eq!(std::fs::read(&out).unwrap(), b"new program");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn swaps_the_executable() {
        let dir = std::env::temp_dir().join(format!("odl-swap-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (exe, new) = (dir.join("app"), dir.join("app.new"));
        std::fs::write(&exe, b"old").unwrap();
        std::fs::write(&new, b"new").unwrap();
        swap_in(&exe, &new).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert!(!new.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
