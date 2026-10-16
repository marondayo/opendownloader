//! Desktop notifications: XDG on Linux, toasts on Windows.

use notify_rust::NotificationResponse;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// What clicking a notification (or one of its buttons) opens.
#[derive(Debug, Clone, PartialEq)]
pub enum NotifyTarget {
    None,
    File(PathBuf),
    Folder(PathBuf),
}

#[derive(Debug, PartialEq)]
enum Action {
    Open(PathBuf),
    ShowInFolder(PathBuf),
}

const OPEN: &str = "open";
const SHOW: &str = "show";
const OPEN_FOLDER: &str = "open-folder";

/// Buttons for `target`, as (action id, label).
fn buttons(target: &NotifyTarget) -> &'static [(&'static str, &'static str)] {
    match target {
        NotifyTarget::None => &[],
        NotifyTarget::File(_) => &[(OPEN, "Open"), (SHOW, "Show in folder")],
        NotifyTarget::Folder(_) => &[(OPEN_FOLDER, "Open folder")],
    }
}

/// What to do for the user's response. A click on the body does what the last button does.
fn action_for(response: &NotificationResponse, target: &NotifyTarget) -> Option<Action> {
    let id = match response {
        NotificationResponse::Default => None,
        NotificationResponse::Action(id) => Some(id.as_str()),
        _ => return None,
    };
    match (target, id) {
        (NotifyTarget::File(p), None | Some(SHOW)) => Some(Action::ShowInFolder(p.clone())),
        (NotifyTarget::File(p), Some(OPEN)) => Some(Action::Open(p.clone())),
        (NotifyTarget::Folder(p), None | Some(OPEN_FOLDER)) => Some(Action::Open(p.clone())),
        _ => None,
    }
}

/// Notifications with buttons keep a thread (and on Linux a D-Bus connection) waiting until they
/// close. GNOME and KDE keep them in the tray, so cap how many wait at once; the rest get no buttons.
const MAX_WAITING: usize = 8;
static WAITING: AtomicUsize = AtomicUsize::new(0);

/// One of the `MAX_WAITING` places for a notification that waits for a click; freed on drop.
struct WaitSlot<'a>(&'a AtomicUsize);

impl<'a> WaitSlot<'a> {
    fn acquire(counter: &'a AtomicUsize, max: usize) -> Option<Self> {
        if counter.fetch_add(1, Ordering::AcqRel) < max {
            Some(WaitSlot(counter))
        } else {
            counter.fetch_sub(1, Ordering::AcqRel);
            None
        }
    }
}

impl Drop for WaitSlot<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// AppUserModelID Windows toasts are sent under (the same id as the Linux .desktop file).
#[cfg(windows)]
const APP_ID: &str = "io.github.opendownloader";

/// Shows a desktop notification (best effort, never blocks the UI). With a target, clicking the
/// notification or one of its buttons opens the file or folder.
pub fn notify(summary: &str, body: &str, target: NotifyTarget) {
    let (summary, body) = (summary.to_string(), body.to_string());
    std::thread::spawn(move || show(&summary, &body, target));
}

fn show(summary: &str, body: &str, target: NotifyTarget) {
    // Without a free slot the notification is shown without buttons and nothing waits for it.
    let slot = if buttons(&target).is_empty() { None } else { WaitSlot::acquire(&WAITING, MAX_WAITING) };
    let mut n = notify_rust::Notification::new();
    n.summary(summary).body(body);
    #[cfg(windows)]
    n.app_id(APP_ID);
    #[cfg(not(windows))]
    n.appname("OpenDownloader").icon("folder-download");
    if slot.is_some() {
        // XDG: "default" is a click on the notification body; servers don't show it as a button.
        #[cfg(not(windows))]
        n.action("default", "Default");
        for (id, label) in buttons(&target) {
            n.action(id, label);
        }
    }
    let handle = match n.show() {
        Ok(handle) => handle,
        Err(e) => {
            tracing::debug!("notification failed: {e}");
            return;
        }
    };
    if slot.is_none() {
        return;
    }
    // Not `wait_for_action`: on Windows it reports a click on the body as "__closed".
    let result = handle.wait_for_response(|response: &NotificationResponse| match action_for(response, &target) {
        Some(Action::Open(path)) => super::open_path(&path),
        Some(Action::ShowInFolder(path)) => super::show_in_folder(&path),
        None => {}
    });
    if let Err(e) = result {
        tracing::debug!("notification response failed: {e}");
    }
}

/// Windows: registers the AppUserModelID toasts are sent under, with a display name and icon, so
/// they show as "OpenDownloader" from both the installer and the portable zip. No-op elsewhere.
pub fn register_notifications() {
    #[cfg(windows)]
    if let Err(e) = win::register() {
        tracing::debug!("could not register for notifications: {e:#}");
    }
}

#[cfg(windows)]
mod win {
    use super::APP_ID;
    use anyhow::{anyhow, bail, Context};
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    };

    pub fn register() -> anyhow::Result<()> {
        let dir = &odl_core::paths::get().data;
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let icon = dir.join("notification-icon.png");
        if !icon.exists() {
            image::save_buffer(&icon, &crate::icon::rgba(256), 256, 256, image::ExtendedColorType::Rgba8)
                .with_context(|| format!("writing {}", icon.display()))?;
        }
        // Rewritten on every start, so a moved data folder fixes itself.
        let key = format!(r"Software\Classes\AppUserModelId\{APP_ID}");
        set_strings(&key, &[("DisplayName", "OpenDownloader"), ("IconUri", &icon.display().to_string())])
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    /// Creates `HKCU\<subkey>` if needed and sets REG_SZ values on it.
    fn set_strings(subkey: &str, values: &[(&str, &str)]) -> anyhow::Result<()> {
        let mut key: HKEY = null_mut();
        let rc = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(subkey).as_ptr(),
                0,
                null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                null(),
                &mut key,
                null_mut(),
            )
        };
        if rc != ERROR_SUCCESS {
            bail!("RegCreateKeyExW({subkey}) failed: error {rc}");
        }
        let mut result = Ok(());
        for (name, value) in values {
            let data = wide(value);
            let rc = unsafe {
                RegSetValueExW(key, wide(name).as_ptr(), 0, REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32)
            };
            if rc != ERROR_SUCCESS {
                result = Err(anyhow!("RegSetValueExW({name}) failed: error {rc}"));
                break;
            }
        }
        unsafe { RegCloseKey(key) };
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify_rust::CloseReason;

    fn file() -> NotifyTarget {
        NotifyTarget::File(PathBuf::from("dir/video.mp4"))
    }
    fn folder() -> NotifyTarget {
        NotifyTarget::Folder(PathBuf::from("dir"))
    }
    fn button(id: &str) -> NotificationResponse {
        NotificationResponse::Action(id.to_string())
    }

    #[test]
    fn file_body_click_and_buttons() {
        let p = PathBuf::from("dir/video.mp4");
        assert_eq!(action_for(&NotificationResponse::Default, &file()), Some(Action::ShowInFolder(p.clone())));
        assert_eq!(action_for(&button(SHOW), &file()), Some(Action::ShowInFolder(p.clone())));
        assert_eq!(action_for(&button(OPEN), &file()), Some(Action::Open(p)));
        assert_eq!(action_for(&button(OPEN_FOLDER), &file()), None);
    }

    #[test]
    fn folder_body_click_and_button() {
        let p = PathBuf::from("dir");
        assert_eq!(action_for(&NotificationResponse::Default, &folder()), Some(Action::Open(p.clone())));
        assert_eq!(action_for(&button(OPEN_FOLDER), &folder()), Some(Action::Open(p)));
        assert_eq!(action_for(&button(SHOW), &folder()), None);
    }

    #[test]
    fn closed_or_no_target_does_nothing() {
        assert_eq!(action_for(&NotificationResponse::Closed(CloseReason::Expired), &file()), None);
        assert_eq!(action_for(&NotificationResponse::Closed(CloseReason::Dismissed), &folder()), None);
        assert_eq!(action_for(&NotificationResponse::Default, &NotifyTarget::None), None);
        assert_eq!(action_for(&button(OPEN), &NotifyTarget::None), None);
    }

    #[test]
    fn every_button_has_an_action() {
        for target in [file(), folder()] {
            for (id, _) in buttons(&target) {
                assert!(action_for(&button(id), &target).is_some(), "{id} does nothing");
            }
        }
        assert!(buttons(&NotifyTarget::None).is_empty());
    }

    #[test]
    fn wait_slots_are_capped_and_freed() {
        let counter = AtomicUsize::new(0);
        let a = WaitSlot::acquire(&counter, 2);
        let b = WaitSlot::acquire(&counter, 2);
        assert!(a.is_some() && b.is_some());
        assert!(WaitSlot::acquire(&counter, 2).is_none());
        drop(a);
        assert!(WaitSlot::acquire(&counter, 2).is_some()); // freed again at the end of this statement
        assert_eq!(counter.load(Ordering::Acquire), 1); // only `b` is left
    }
}
