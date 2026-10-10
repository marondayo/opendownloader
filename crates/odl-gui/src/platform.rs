//! Desktop integration: opening files/folders/links and notifications.

use std::path::{Path, PathBuf};

pub fn open_path(path: &Path) {
    if let Err(e) = open::that_detached(path) {
        tracing::warn!("could not open {}: {e}", path.display());
    }
}

pub fn open_url(url: &str) {
    if let Err(e) = open::that_detached(url) {
        tracing::warn!("could not open {url}: {e}");
    }
}

/// Reveals a file in the file manager, selecting it when the file manager supports it.
pub fn show_in_folder(path: &Path) {
    let path: PathBuf = path.to_path_buf();
    std::thread::spawn(move || {
        #[cfg(target_os = "linux")]
        if path.is_file() {
            if let Ok(uri) = url::Url::from_file_path(&path) {
                let ok = std::process::Command::new("dbus-send")
                    .args([
                        "--session",
                        "--print-reply",
                        "--dest=org.freedesktop.FileManager1",
                        "--type=method_call",
                        "/org/freedesktop/FileManager1",
                        "org.freedesktop.FileManager1.ShowItems",
                        &format!("array:string:{uri}"),
                        "string:",
                    ])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                if ok {
                    return;
                }
            }
        }
        #[cfg(windows)]
        if path.is_file() {
            let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
            return;
        }
        let dir = if path.is_dir() { path.as_path() } else { path.parent().unwrap_or(&path) };
        open_path(dir);
    });
}

/// Shows a desktop notification (best effort, never blocks the UI).
pub fn notify(summary: &str, body: &str) {
    #[cfg(target_os = "linux")]
    {
        let (summary, body) = (summary.to_string(), body.to_string());
        std::thread::spawn(move || {
            if let Err(e) = notify_rust::Notification::new()
                .appname("OpenDownloader")
                .summary(&summary)
                .body(&body)
                .icon("folder-download")
                .show()
            {
                tracing::debug!("notification failed: {e}");
            }
        });
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (summary, body);
}
