#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

//! OpenDownloader: a free, open-source video & audio downloader.

mod app;
mod fonts;
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

/// Draws the app icon: a green rounded square with a white download arrow.
fn app_icon(size: u32) -> egui::IconData {
    let s = size as f32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let (margin, radius) = (s * 0.06, s * 0.22);
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // Rounded-rectangle coverage via distance to the inner rectangle.
            let (lo, hi) = (margin + radius, s - margin - radius);
            let dx = (lo - fx).max(fx - hi).max(0.0);
            let dy = (lo - fy).max(fy - hi).max(0.0);
            let dist = (dx * dx + dy * dy).sqrt() - radius;
            let alpha = (0.5 - dist).clamp(0.0, 1.0);
            if alpha <= 0.0 {
                continue;
            }
            let t = fy / s;
            let mut c = [(104.0 - 30.0 * t) as u8, (196.0 - 40.0 * t) as u8, (84.0 - 24.0 * t) as u8];
            let cx = s * 0.5;
            let shaft = (fx - cx).abs() <= s * 0.075 && fy >= s * 0.2 && fy <= s * 0.5;
            let head = fy >= s * 0.42 && fy <= s * 0.66 && (fx - cx).abs() <= (s * 0.66 - fy) * 0.95;
            let base = fy >= s * 0.74 && fy <= s * 0.82 && fx >= s * 0.27 && fx <= s * 0.73;
            if shaft || head || base {
                c = [255, 255, 255];
            }
            let i = ((y * size + x) * 4) as usize;
            rgba[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], (alpha * 255.0) as u8]);
        }
    }
    egui::IconData { rgba, width: size, height: size }
}
