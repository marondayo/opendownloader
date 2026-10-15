//! Optional YouTube PO-token provider ([bgutil-ytdlp-pot-provider]).
//!
//! YouTube's mobile-web client (`mweb`) only serves full-quality formats together with a
//! proof-of-origin (PO) token minted by Google's BotGuard. bgutil is a yt-dlp plugin plus a
//! small script that runs BotGuard under Deno. When it is installed, YouTube extraction also
//! asks `mweb`, which rescues videos the embed player can't serve (embedding disabled) while
//! the default clients are stuck behind a bot check.
//!
//! Layout in the data dir: `bgutil/plugins/<plugin>.zip` (loaded with `--plugin-dirs`) and
//! `bgutil/server/` (the script, with its npm dependencies installed by Deno).
//!
//! [bgutil-ytdlp-pot-provider]: https://github.com/Brainicism/bgutil-ytdlp-pot-provider

use crate::process::command;
use crate::tools::{download_file, http, InstallProgress};
use anyhow::{anyhow, bail, Context};
use std::path::{Path, PathBuf};

const REPO: &str = "Brainicism/bgutil-ytdlp-pot-provider";
const PLUGIN_ZIP: &str = "bgutil-ytdlp-pot-provider.zip";

/// Where the provider lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PotProvider {
    pub plugins_dir: PathBuf,
    pub server_dir: PathBuf,
}

impl PotProvider {
    fn at(root: &Path) -> Self {
        Self { plugins_dir: root.join("plugins"), server_dir: root.join("server") }
    }

    /// yt-dlp arguments that load the plugin and point its script mode at the server.
    pub fn ytdlp_args(&self) -> Vec<String> {
        vec![
            "--plugin-dirs".into(),
            self.plugins_dir.to_string_lossy().into_owned(),
            "--extractor-args".into(),
            format!("youtubepot-bgutilscript:server_home={}", self.server_dir.to_string_lossy()),
        ]
    }
}

fn root() -> PathBuf {
    crate::paths::get().data.join("bgutil")
}

fn version_file(root: &Path) -> PathBuf {
    root.join("version")
}

/// The managed provider, if completely installed.
pub fn installed() -> Option<PotProvider> {
    let root = root();
    let p = PotProvider::at(&root);
    let complete = version_file(&root).is_file()
        && p.plugins_dir.join(PLUGIN_ZIP).is_file()
        && p.server_dir.join("node_modules").is_dir();
    complete.then_some(p)
}

/// Installed provider version (release tag).
pub fn installed_version() -> Option<String> {
    installed()?;
    std::fs::read_to_string(version_file(&root())).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Latest provider release tag (e.g. `2.0.2`).
pub async fn latest_version() -> anyhow::Result<String> {
    #[derive(serde::Deserialize)]
    struct Release {
        tag_name: String,
    }
    let r: Release = http()?
        .get(format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(r.tag_name)
}

/// Extracts the `server/` folder of a GitHub source zipball (`<repo>-<tag>/server/...`) into `dest`.
fn extract_server(archive: &Path, dest: &Path) -> anyhow::Result<usize> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
    let mut count = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(path) = entry.enclosed_name() else { continue };
        let mut parts = path.components();
        parts.next(); // `<repo>-<tag>/`
        let Ok(rel) = parts.as_path().strip_prefix("server") else { continue };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else if entry.is_file() {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::io::copy(&mut entry, &mut std::fs::File::create(&out)?)?;
            count += 1;
        }
    }
    Ok(count)
}

/// Downloads the latest provider and installs its dependencies with `deno`.
/// Replaces any existing copy. Returns the installed version.
pub async fn install(deno: &Path, progress: impl Fn(InstallProgress) + Send + Sync + 'static) -> anyhow::Result<String> {
    let tag = latest_version().await.context("looking up the latest release")?;
    let root = root();
    // Source + npm dependencies (including the native canvas module) take ~250 MB.
    let needed: u64 = 400 << 20;
    if let Some(free) = crate::disk::free_space(root.parent().unwrap_or(&root)) {
        if free < needed {
            bail!(
                "not enough disk space: needs about {}, {} free",
                crate::model::format_bytes(needed),
                crate::model::format_bytes(free)
            );
        }
    }
    // Built in place: node_modules may hold links that would break if the folder moved.
    remove()?;
    let result = install_into(&root, &tag, deno, &progress).await;
    if result.is_err() {
        let _ = remove();
    }
    result.map(|_| tag)
}

async fn install_into(
    root: &Path,
    tag: &str,
    deno: &Path,
    progress: &(dyn Fn(InstallProgress) + Send + Sync),
) -> anyhow::Result<()> {
    let p = PotProvider::at(root);
    tokio::fs::create_dir_all(&p.plugins_dir).await?;
    tokio::fs::create_dir_all(&p.server_dir).await?;

    // Plugin and script must come from the same release.
    let plugin_url = format!("https://github.com/{REPO}/releases/download/{tag}/{PLUGIN_ZIP}");
    download_file(&plugin_url, &p.plugins_dir.join(PLUGIN_ZIP), progress)
        .await
        .with_context(|| format!("downloading {plugin_url}"))?;

    let source_url = format!("https://github.com/{REPO}/archive/refs/tags/{tag}.zip");
    let source_zip = root.join("source.zip");
    download_file(&source_url, &source_zip, progress).await.with_context(|| format!("downloading {source_url}"))?;
    progress(InstallProgress::Extracting);
    let (zip2, server2) = (source_zip.clone(), p.server_dir.clone());
    let files = tokio::task::spawn_blocking(move || extract_server(&zip2, &server2)).await??;
    let _ = tokio::fs::remove_file(&source_zip).await;
    if files == 0 {
        bail!("the source archive has no server folder");
    }

    progress(InstallProgress::Installing);
    let mut cmd = command(deno);
    cmd.args(["install", "--allow-scripts=npm:canvas", "--frozen"]).current_dir(&p.server_dir);
    let out = tokio::time::timeout(std::time::Duration::from_secs(600), cmd.output())
        .await
        .map_err(|_| anyhow!("installing dependencies timed out"))?
        .context("could not run deno")?;
    if !out.status.success() {
        let err = crate::process::strip_ansi(&String::from_utf8_lossy(&out.stderr));
        let tail: Vec<&str> = err.lines().rev().take(6).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        bail!("installing dependencies failed:\n{}", tail.join("\n"));
    }
    tokio::fs::write(version_file(root), tag).await?;
    Ok(())
}

/// Deletes the managed provider.
pub fn remove() -> anyhow::Result<()> {
    let root = root();
    if root.exists() {
        std::fs::remove_dir_all(&root).with_context(|| format!("removing {}", root.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn args_point_at_plugin_and_server() {
        let p = PotProvider::at(Path::new("/data/bgutil"));
        let a = p.ytdlp_args();
        assert_eq!(a[0], "--plugin-dirs");
        assert!(a[1].ends_with("plugins"));
        assert_eq!(a[2], "--extractor-args");
        assert!(a[3].starts_with("youtubepot-bgutilscript:server_home="));
        assert!(a[3].ends_with("server"));
    }

    #[test]
    fn extracts_only_server_folder() {
        let dir = std::env::temp_dir().join(format!("odl-pot-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("src.zip");
        {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            for (name, body) in [
                ("repo-2.0.2/README.md", "readme"),
                ("repo-2.0.2/server/deno.json", "{}"),
                ("repo-2.0.2/server/src/main.ts", "main"),
                ("repo-2.0.2/plugin/x.py", "py"),
            ] {
                zip.start_file(name, opts).unwrap();
                zip.write_all(body.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        let dest = dir.join("server");
        assert_eq!(extract_server(&archive, &dest).unwrap(), 2);
        assert_eq!(std::fs::read_to_string(dest.join("src/main.ts")).unwrap(), "main");
        assert!(dest.join("deno.json").is_file());
        assert!(!dest.join("README.md").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
