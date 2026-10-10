//! Platform-specific locations for config, data and cache.

use directories::{ProjectDirs, UserDirs};
use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
}

static PATHS: OnceLock<AppPaths> = OnceLock::new();

/// Returns the application directories.
///
/// Set `OPENDOWNLOADER_HOME` to keep everything under a single folder (portable mode).
pub fn get() -> &'static AppPaths {
    PATHS.get_or_init(|| {
        if let Some(home) = std::env::var_os("OPENDOWNLOADER_HOME") {
            let base = PathBuf::from(home);
            return AppPaths {
                config: base.join("config"),
                data: base.join("data"),
                cache: base.join("cache"),
            };
        }
        match ProjectDirs::from("io.github", "OpenDownloader", "opendownloader") {
            Some(pd) => AppPaths {
                config: pd.config_dir().to_path_buf(),
                data: pd.data_dir().to_path_buf(),
                cache: pd.cache_dir().to_path_buf(),
            },
            None => {
                let base = PathBuf::from(".opendownloader");
                AppPaths {
                    config: base.join("config"),
                    data: base.join("data"),
                    cache: base.join("cache"),
                }
            }
        }
    })
}

impl AppPaths {
    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.json")
    }
    pub fn history_file(&self) -> PathBuf {
        self.data.join("history.json")
    }
    pub fn netrc_file(&self) -> PathBuf {
        self.config.join("netrc")
    }
    pub fn cookies_dir(&self) -> PathBuf {
        self.config.join("cookies")
    }
    pub fn bin_dir(&self) -> PathBuf {
        self.data.join("bin")
    }
    pub fn archive_file(&self) -> PathBuf {
        self.data.join("download-archive.txt")
    }
    pub fn temp_dir(&self) -> PathBuf {
        self.cache.join("tmp")
    }
}

/// Default folder for downloads: `~/Downloads/OpenDownloader`.
pub fn default_download_dir() -> PathBuf {
    UserDirs::new()
        .map(|u| {
            u.download_dir()
                .map(|d| d.to_path_buf())
                .unwrap_or_else(|| u.home_dir().join("Downloads"))
        })
        .unwrap_or_else(|| PathBuf::from("."))
        .join("OpenDownloader")
}
