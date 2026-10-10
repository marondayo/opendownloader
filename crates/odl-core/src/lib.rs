//! Download engine for OpenDownloader.
//!
//! The heavy lifting (site extraction, DASH/HLS handling, muxing) is delegated to
//! [yt-dlp](https://github.com/yt-dlp/yt-dlp) and [ffmpeg](https://ffmpeg.org).
//! This crate orchestrates them: it classifies links, probes metadata, runs a
//! download queue with live progress, converts media, manages per-site accounts
//! and keeps the external tools installed and up to date.

pub mod auth;
pub mod convert;
pub mod manager;
pub mod model;
pub mod paths;
pub mod process;
pub mod settings;
pub mod site;
pub mod store;
pub mod tools;
pub mod ytdlp;

pub use manager::{EngineEvent, Job, JobOutcome, JobSpec, Manager};
pub use model::*;
pub use settings::Settings;
pub use site::{parse_link, Link, LinkError, LinkKind, Site};
pub use tools::ToolPaths;

/// Everything a job needs to run: a snapshot of the settings and resolved tool paths.
#[derive(Debug, Clone, Default)]
pub struct RunCtx {
    pub settings: Settings,
    pub tools: ToolPaths,
}

/// Error type for long-running jobs, separating user cancellation from failure.
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Failed(String),
}

impl From<anyhow::Error> for JobError {
    fn from(e: anyhow::Error) -> Self {
        JobError::Failed(format!("{e:#}"))
    }
}

impl From<std::io::Error> for JobError {
    fn from(e: std::io::Error) -> Self {
        JobError::Failed(e.to_string())
    }
}
