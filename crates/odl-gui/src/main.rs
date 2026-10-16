#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

//! OpenDownloader: a free, open-source video & audio downloader.

mod app;
mod clipboard;
mod fonts;
mod icon;
mod items;
mod platform;
mod theme;
mod views;
mod widgets;

use eframe::egui;
use std::sync::Arc;

fn main() -> eframe::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,yt-dlp=info".into()),
        )
        .init();
    platform::register_notifications();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OpenDownloader")
            .with_app_id("io.github.opendownloader")
            .with_inner_size([1060.0, 700.0])
            .with_min_inner_size([760.0, 480.0])
            .with_drag_and_drop(true)
            .with_icon(Arc::new(app_icon(128))),
        ..Default::default()
    };
    eframe::run_native("OpenDownloader", options, Box::new(|cc| Ok(Box::new(app::OdlApp::new(cc)))))
}

fn app_icon(size: u32) -> egui::IconData {
    egui::IconData { rgba: icon::rgba(size), width: size, height: size }
}
