//! Desktop notifications: XDG on Linux, toasts on Windows.

/// AppUserModelID Windows toasts are sent under (the same id as the Linux .desktop file).
#[cfg(windows)]
const APP_ID: &str = "io.github.opendownloader";

/// Shows a desktop notification (best effort, never blocks the UI).
pub fn notify(summary: &str, body: &str) {
    let (summary, body) = (summary.to_string(), body.to_string());
    std::thread::spawn(move || {
        let mut n = notify_rust::Notification::new();
        n.summary(&summary).body(&body);
        #[cfg(windows)]
        n.app_id(APP_ID);
        #[cfg(not(windows))]
        n.appname("OpenDownloader").icon("folder-download");
        if let Err(e) = n.show() {
            tracing::debug!("notification failed: {e}");
        }
    });
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
