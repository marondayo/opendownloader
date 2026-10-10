//! Clipboard monitoring: notices when a supported link is copied anywhere on the desktop.

use crate::app::{Bus, UiMsg};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const POLL: Duration = Duration::from_millis(800);
/// Longer texts are documents, not links; don't bother parsing them.
const MAX_LEN: usize = 2048;

/// Starts the watcher thread. `enabled` can be flipped at any time from the settings.
pub fn spawn(bus: Bus, enabled: Arc<AtomicBool>) {
    let result = std::thread::Builder::new().name("odl-clipboard".into()).spawn(move || {
        let mut clipboard: Option<arboard::Clipboard> = None;
        let mut last: Option<String> = None;
        // Whatever is on the clipboard when watching starts is old news.
        let mut primed = false;
        loop {
            std::thread::sleep(POLL);
            if !enabled.load(Ordering::Relaxed) {
                primed = false;
                continue;
            }
            if clipboard.is_none() {
                match arboard::Clipboard::new() {
                    Ok(c) => clipboard = Some(c),
                    Err(e) => {
                        tracing::debug!("clipboard unavailable: {e}");
                        std::thread::sleep(Duration::from_secs(5));
                        continue;
                    }
                }
            }
            let Some(cb) = clipboard.as_mut() else { continue };
            let text = match cb.get_text() {
                Ok(t) => t.trim().to_string(),
                // Empty clipboard or non-text content.
                Err(_) => String::new(),
            };
            if last.as_deref() == Some(text.as_str()) {
                primed = true;
                continue;
            }
            let was_primed = primed;
            last = Some(text.clone());
            primed = true;
            if was_primed && !text.is_empty() && text.len() <= MAX_LEN && odl_core::parse_link(&text).is_ok() {
                bus.send(UiMsg::ClipboardLink(text));
            }
        }
    });
    if let Err(e) = result {
        tracing::warn!("could not start clipboard watcher: {e}");
    }
}
