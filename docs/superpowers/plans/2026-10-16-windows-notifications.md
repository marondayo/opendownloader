# Windows Notifications Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Desktop notifications work on Windows, and on Windows and Linux a notification can be clicked (or its **Open** / **Show in folder** / **Open folder** buttons used) to open the downloaded file or folder.

**Architecture:** notify-rust on both platforms, in a new `platform/notification.rs`. On Windows the app registers an AppUserModelID in `HKCU\Software\Classes\AppUserModelId` at startup (display name + icon), so the installer and the portable zip both get proper toasts. A background thread per notification waits for the user's response, and a pure function maps it to `open_path` / `show_in_folder`.

**Tech Stack:** Rust 1.95, eframe/egui 0.36, notify-rust 4.18.2 (XDG on Linux, `tauri-winrt-notification` on Windows), windows-sys 0.61, Inno Setup 6.

**Spec:** `docs/superpowers/specs/2026-10-16-windows-notifications-design.md`

---

## Ground rules

- **Git identity:** before every commit, run `git config user.name` in the checkout. It must print `marondayo`; stop if it doesn't. End every commit message with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Windows build/test** (repo root, Git Bash or PowerShell): `cargo +1.95 test -p odl-gui`
- **Linux build/test** (WSL, from Windows):
  `wsl -d Ubuntu-24.04 -- bash -lc 'cd /mnt/c/Users/sinta/projects/opendownloader-pro && CARGO_TARGET_DIR=~/odl-target cargo +1.95 test -p odl-gui'`
- **Clippy:** same two commands with `clippy -p odl-gui --all-targets` instead of `test -p odl-gui`. Expect no new warnings in the files this plan touches.
- **GUI checks are done by the user.** Never screenshot or drive the desktop. Ask the user to run the app and report.

## File structure

| File | Change | Responsibility |
|---|---|---|
| `crates/odl-gui/src/platform.rs` → `crates/odl-gui/src/platform/mod.rs` | move, then edit | open file/URL, show in folder; re-exports notification API |
| `crates/odl-gui/src/platform/notification.rs` | create | `notify`, `NotifyTarget`, action mapping, wait cap, Windows AUMID registration |
| `crates/odl-gui/Cargo.toml` | modify | notify-rust for all targets; windows-sys on Windows |
| `crates/odl-gui/src/main.rs` | modify | call `platform::register_notifications()` at startup |
| `crates/odl-gui/src/app.rs` | modify | pass a `NotifyTarget` at the 3 call sites |
| `crates/odl-gui/src/views/settings.rs` | modify | test notification passes `NotifyTarget::None` |
| `packaging/windows/opendownloader.iss` | modify | delete the AUMID key on uninstall |
| `docs/TESTING.md` | modify | manual checks for Windows toasts and Linux actions |

---

### Task 1: Move `platform.rs` into a folder and make notify-rust cross-platform

No behaviour change. It sets up the module for the next tasks.

**Files:**
- Move: `crates/odl-gui/src/platform.rs` → `crates/odl-gui/src/platform/mod.rs`
- Modify: `crates/odl-gui/Cargo.toml`

- [ ] **Step 1: Move the file**

```bash
mkdir -p crates/odl-gui/src/platform
git mv crates/odl-gui/src/platform.rs crates/odl-gui/src/platform/mod.rs
```

- [ ] **Step 2: Make notify-rust a dependency on every platform**

In `crates/odl-gui/Cargo.toml`, add the line below directly after `open = "5"` in `[dependencies]`:

```toml
notify-rust = "4"
```

and delete these two lines (the Linux-only table):

```toml
[target.'cfg(target_os = "linux")'.dependencies]
notify-rust = "4"
```

- [ ] **Step 3: Build on Windows and Linux**

Run the Windows and Linux test commands from *Ground rules*.
Expected: both build; all existing tests PASS. `Cargo.lock` changes only if cargo re-sorts odl-gui's dependency list. That's fine; commit it.

- [ ] **Step 4: Commit**

```bash
git add -A crates/odl-gui Cargo.lock
git commit -F - <<'EOF'
Notifications: move platform.rs into a folder, notify-rust on all platforms

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 2: Windows toasts under a registered AppUserModelID (display only)

This is the riskiest assumption in the spec: that a registry-only AUMID gives toasts the right name and icon without a Start-menu shortcut. It is built and checked by the user **before** the action work.

**Files:**
- Create: `crates/odl-gui/src/platform/notification.rs`
- Modify: `crates/odl-gui/src/platform/mod.rs` (lines 70-89: the old `notify`)
- Modify: `crates/odl-gui/Cargo.toml`
- Modify: `crates/odl-gui/src/main.rs:18-23`

- [ ] **Step 1: Add windows-sys for Windows builds**

In `crates/odl-gui/Cargo.toml`, add this table directly above `[dev-dependencies]`:

```toml
[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.61", features = ["Win32_Foundation", "Win32_Security", "Win32_System_Registry"] }
```

(`Win32_Security` is required: `RegCreateKeyExW` takes a `SECURITY_ATTRIBUTES` pointer and is gated on that feature.)

- [ ] **Step 2: Create `platform/notification.rs`**

```rust
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
```

- [ ] **Step 3: Replace the old `notify` in `platform/mod.rs`**

Keep the `//!` doc comment on line 1 and insert directly below it (before `use std::path::...`):

```rust
mod notification;

pub use notification::{notify, register_notifications};
```

and delete the old function at the end of the file (the whole block starting at
`/// Shows a desktop notification (best effort, never blocks the UI).` through the closing `}` of `pub fn notify`).

- [ ] **Step 4: Register at startup**

In `crates/odl-gui/src/main.rs`, directly after the `tracing_subscriber::fmt()...init();` statement in `main`, add:

```rust
    platform::register_notifications();
```

- [ ] **Step 5: Build and test on Windows and Linux**

Run the Windows and Linux test commands, then both clippy commands.
Expected: build OK, existing tests PASS, no new clippy warnings.

- [ ] **Step 6: USER CHECKPOINT: registry AUMID works**

Ask the user to run on Windows:

```bash
cargo +1.95 run -p odl-gui
```

then **Settings → Notifications → Send a test notification**. Expected: a toast labelled **OpenDownloader** with the green download-arrow icon, *not* "Windows PowerShell" and not nothing. Also ask them to confirm with:

```bash
reg query "HKCU\Software\Classes\AppUserModelId\io.github.opendownloader"
```

which should list `DisplayName` and `IconUri`.

**If no toast appears:** add `Win32_UI_Shell` to the windows-sys features, and in `win::register` call
`windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(wide(APP_ID).as_ptr())` (check the
`HRESULT` is `>= 0`). Then, in Task 4, also add `AppUserModelID: "io.github.opendownloader"` to both
`[Icons]` lines in the installer so pinned shortcuts group with the window. Re-check with the user.
Do **not** continue to Task 3 until the user confirms toasts show correctly.

- [ ] **Step 7: Commit**

```bash
git add -A crates/odl-gui Cargo.lock
git commit -F - <<'EOF'
Notifications: show toasts on Windows under a registered app id

The app registers io.github.opendownloader in HKCU\Software\Classes\AppUserModelId
(display name + icon) at startup, so the installer and the portable zip both work.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 3: Open / Show in folder actions (both platforms)

**Files:**
- Modify: `crates/odl-gui/src/platform/notification.rs`
- Modify: `crates/odl-gui/src/platform/mod.rs` (the `pub use` line)
- Modify: `crates/odl-gui/src/app.rs:651-660`, `:690-697`, `:951-957`
- Modify: `crates/odl-gui/src/views/settings.rs:431`

- [ ] **Step 1: Write the failing tests**

Append to `crates/odl-gui/src/platform/notification.rs`:

```rust
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
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo +1.95 test -p odl-gui platform::notification`
Expected: FAIL to compile with errors such as "cannot find type `NotifyTarget`", "cannot find function `action_for`", "cannot find value `SHOW`", "cannot find type `WaitSlot`".

- [ ] **Step 3: Add the pure logic**

In `crates/odl-gui/src/platform/notification.rs`, directly below the `//!` doc comment, add:

```rust
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
        counter
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < max).then_some(n + 1))
            .ok()
            .map(|_| WaitSlot(counter))
    }
}

impl Drop for WaitSlot<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo +1.95 test -p odl-gui platform::notification`
Expected: 5 tests PASS. Dead-code warnings for `MAX_WAITING`/`WAITING` are expected until Step 5; **do not commit yet**.

- [ ] **Step 5: Wire the actions into `notify`**

In `crates/odl-gui/src/platform/notification.rs`, replace the whole `pub fn notify` from Task 2 with:

```rust
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
```

In `crates/odl-gui/src/platform/mod.rs`, change the re-export to:

```rust
pub use notification::{notify, register_notifications, NotifyTarget};
```

- [ ] **Step 6: Update the test-notification button**

In `crates/odl-gui/src/views/settings.rs` (in `page_notifications`), replace:

```rust
            crate::platform::notify("OpenDownloader", "Notifications are working.");
```

with:

```rust
            crate::platform::notify("OpenDownloader", "Notifications are working.", crate::platform::NotifyTarget::None);
```

- [ ] **Step 7: Download / conversion finished → `File`**

In `crates/odl-gui/src/app.rs` (engine `Completed` handling, around line 651), replace:

```rust
                let (title, parent, kind) = (item.title.clone(), item.parent, item.kind);
```

with:

```rust
                let (title, parent, kind, file) = (item.title.clone(), item.parent, item.kind, item.file_path.clone());
```

and replace:

```rust
                        platform::notify(summary, &title);
```

with:

```rust
                        let target = file.map_or(platform::NotifyTarget::None, platform::NotifyTarget::File);
                        platform::notify(summary, &title, target);
```

- [ ] **Step 8: Playlist downloaded → `Folder(group.output_dir)`**

In `on_group_changed` (around line 690), replace:

```rust
        let title = group.title.clone();
```

with:

```rust
        let (title, dir) = (group.title.clone(), group.output_dir.clone());
```

and replace:

```rust
            platform::notify("Playlist downloaded", &format!("{title} ({count} items)"));
```

with:

```rust
            platform::notify("Playlist downloaded", &format!("{title} ({count} items)"), platform::NotifyTarget::Folder(dir));
```

- [ ] **Step 9: Subscription → `Folder` of the new downloads**

In the subscription check (around line 951), replace:

```rust
                if count > 0 {
                    self.start_from_probe_ext(&ProbeResult::Playlist(pl), Some(site), Some(&mask), options, false);
                    if self.settings.notify_download_finished {
                        let noun = if count == 1 { "new video" } else { "new videos" };
                        platform::notify("Subscription", &format!("{count} {noun} from {title}"));
                    }
                }
```

with:

```rust
                if count > 0 {
                    // The same folder start_from_probe_ext puts the new downloads in.
                    let dir = if self.settings.playlist_subdirs {
                        options.output_dir.join(sanitize_filename(&pl.title))
                    } else {
                        options.output_dir.clone()
                    };
                    self.start_from_probe_ext(&ProbeResult::Playlist(pl), Some(site), Some(&mask), options, false);
                    if self.settings.notify_download_finished {
                        let noun = if count == 1 { "new video" } else { "new videos" };
                        platform::notify("Subscription", &format!("{count} {noun} from {title}"), platform::NotifyTarget::Folder(dir));
                    }
                }
```

- [ ] **Step 10: Build, test and lint on both platforms**

Run the Windows and Linux test commands, then both clippy commands.
Expected: all tests PASS (including the 5 new ones on both platforms), no warnings in `platform/`, `app.rs` or `settings.rs`.

- [ ] **Step 11: Commit**

```bash
git add -A crates/odl-gui
git commit -F - <<'EOF'
Notifications: Open / Show in folder actions on Windows and Linux

Clicking a download-finished notification shows the file in its folder; buttons open the
file or show it. Playlist and subscription notifications open the folder. At most 8
notifications wait for a click at once; later ones are shown without buttons.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 4: Installer removes the registry key on uninstall

**Files:**
- Modify: `packaging/windows/opendownloader.iss` (new section between `[Icons]` and `[Run]`)

- [ ] **Step 1: Add the `[Registry]` section**

Insert after the two `[Icons]` lines and before `[Run]`:

```ini
[Registry]
; The app writes this key at startup (so the portable zip gets notifications too); remove it on uninstall.
Root: HKCU; Subkey: "Software\Classes\AppUserModelId\io.github.opendownloader"; Flags: uninsdeletekey dontcreatekey
```

`[Code]` must stay the last section (see commit bc06aa2).

- [ ] **Step 2: Compile the installer if Inno Setup is available**

Inno Setup is not installed on this machine; CI's release workflow compiles it. If it is available:

```bash
"/c/Program Files (x86)/Inno Setup 6/ISCC.exe" /DAppVersion=0.1.3 packaging/windows/opendownloader.iss
```

Expected: `Successful compile`. Otherwise read the section once more: `Root`, `Subkey` and `Flags` spelled as above, inside quotes where shown.

- [ ] **Step 3: Commit**

```bash
git add packaging/windows/opendownloader.iss
git commit -F - <<'EOF'
Windows installer: remove the notification app id key on uninstall

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 5: Manual test checklist

**Files:**
- Modify: `docs/TESTING.md` (section 9 "Desktop integration", section 11 "Windows app")

- [ ] **Step 1: Linux actions in section 9**

Directly after the line `- [x] Desktop notifications (Settings → Notifications → Send a test notification).`, add:

```markdown
- [ ] Download-finished notification: click it → file selected in the file manager; **Open** plays it;
      **Show in folder** selects it (GNOME and KDE). Playlist / subscription notifications → **Open folder**.
```

- [ ] **Step 2: Windows checks in section 11**

Replace the line:

```markdown
- [ ] Known gap: desktop notifications are Linux-only for now.
```

with:

```markdown
- [ ] Settings → Notifications → Send a test notification → toast labelled **OpenDownloader** with the green
      icon, from the installer build *and* the portable zip.
- [ ] Download-finished toast: click → Explorer with the file selected; **Open** plays it; **Show in folder**
      selects it. Playlist / subscription toast → **Open folder**. (Clicks work only while the popup is on
      screen, not from the notification center.)
- [ ] Uninstall → `reg query "HKCU\Software\Classes\AppUserModelId\io.github.opendownloader"` finds nothing.
```

- [ ] **Step 3: Commit**

```bash
git add docs/TESTING.md
git commit -F - <<'EOF'
Test checklist: Windows notifications and notification actions

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 6: Final verification

- [ ] **Step 1: Full workspace tests on both platforms**

```bash
cargo +1.95 test --workspace
```

and in WSL:

```bash
wsl -d Ubuntu-24.04 -- bash -lc 'cd /mnt/c/Users/sinta/projects/opendownloader-pro && CARGO_TARGET_DIR=~/odl-target cargo +1.95 test --workspace'
```

Expected: everything PASSES on both.

- [ ] **Step 2: USER CHECKPOINT: GUI pass**

Ask the user to run `cargo +1.95 run -p odl-gui` on Windows (and the Linux build on a real desktop if they want to test it there), download one short video, one small playlist, and go through the new `docs/TESTING.md` items. Record their results; fix anything that fails before finishing the branch.
