//! JSON persistence for settings and the download history.

use crate::model::{DownloadItem, ItemStatus};
use crate::settings::Settings;
use anyhow::Context;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::Path;

/// Loads JSON from `path`. A corrupt file is moved aside (`*.bak`) and the default returned.
pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    let Ok(text) = std::fs::read_to_string(path) else {
        return T::default();
    };
    match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("could not parse {}: {e}; starting fresh", path.display());
            let _ = std::fs::rename(path, path.with_extension("json.bak"));
            T::default()
        }
    }
}

/// Atomically writes JSON (write to a temp file, then rename).
pub fn save_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_vec_pretty(value)?;
    std::fs::write(&tmp, data).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

pub fn load_settings() -> Settings {
    load_json(&crate::paths::get().settings_file())
}

pub fn save_settings(settings: &Settings) -> anyhow::Result<()> {
    save_json(&crate::paths::get().settings_file(), settings)
}

/// Loads the download list. Items that were running when the app quit come back paused.
pub fn load_history() -> Vec<DownloadItem> {
    let mut items: Vec<DownloadItem> = load_json(&crate::paths::get().history_file());
    for item in &mut items {
        if item.status.is_active() {
            item.status = ItemStatus::Paused;
        }
    }
    items
}

pub fn save_history(items: &[DownloadItem]) -> anyhow::Result<()> {
    save_json(&crate::paths::get().history_file(), items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_corrupt_file() {
        let dir = std::env::temp_dir().join(format!("odl-store-test-{}", std::process::id()));
        let path = dir.join("x.json");
        save_json(&path, &vec![1, 2, 3]).unwrap();
        let v: Vec<i32> = load_json(&path);
        assert_eq!(v, vec![1, 2, 3]);
        std::fs::write(&path, "{not json").unwrap();
        let v: Vec<i32> = load_json(&path);
        assert!(v.is_empty());
        assert!(path.with_extension("json.bak").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
