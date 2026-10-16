# Desktop notifications on Windows, with actions

Date: 2026-10-16 · Branch: `windows-notifications`

## Goal

Desktop notifications work on Windows (today `platform::notify` is a no-op there), and on both
Windows and Linux a finished-download notification can be clicked to show the file, with
**Open** / **Show in folder** buttons.

## Current state

- `crates/odl-gui/src/platform.rs` → `notify(summary, body)` spawns a thread and shows a notify-rust
  notification on Linux only; other platforms ignore it.
- `notify-rust = "4"` is a Linux-only dependency of `odl-gui`. The locked version, 4.18.2, also has a
  Windows backend (`tauri-winrt-notification` 0.8.1) that supports buttons and activation.
- Call sites in `app.rs`: download/conversion finished (single item), playlist downloaded,
  subscription found new videos. `views/settings.rs` has "Send a test notification".
- Windows ships as an Inno Setup installer **and** a portable zip.

## Decisions

- **One code path:** notify-rust on both platforms (approach A). No direct `tauri-winrt-notification` use.
- **Windows identity via the registry**, not a shortcut, so the portable zip gets proper toasts too.
- The process's own AppUserModelID is **not** changed, so taskbar grouping and existing pins stay as they are.
- Actions only open files or folders, so they never need to reach back into app state.

## API

`platform.rs` becomes `platform/mod.rs` (open/show-in-folder, unchanged) plus
`platform/notification.rs`:

```rust
pub enum NotifyTarget { None, File(PathBuf), Folder(PathBuf) }

/// Shows a desktop notification (best effort, never blocks the UI).
pub fn notify(summary: &str, body: &str, target: NotifyTarget);

/// Windows: registers the AppUserModelID toasts are sent under. No-op elsewhere.
pub fn register_notifications();
```

`notify` is re-exported from `platform` so call sites stay `platform::notify(...)`.

## Behaviour

| Notification | Target | Body click | Buttons |
|---|---|---|---|
| Download finished / Conversion finished | `File(item.file_path)` | Show in folder | **Open**, **Show in folder** |
| Playlist downloaded | `Folder(group.output_dir)` | Open folder | **Open folder** |
| Subscription: N new videos | `Folder(dir)` | Open folder | **Open folder** |
| Test notification | `None` | nothing | none |

`group.output_dir` already includes the playlist subfolder when `playlist_subdirs` is on. For a
subscription, `dir` uses the same rule as `start_from_probe_ext`: `options.output_dir`, joined with
`sanitize_filename(title)` when `playlist_subdirs` is on.

If an item has no `file_path`, it uses `NotifyTarget::None`. If the file or folder no longer
exists when the user clicks, the existing `open_path` / `show_in_folder` behaviour applies
(a logged warning, nothing else).

## Flow

1. `notify` spawns a thread (as today) that builds a `notify_rust::Notification` with summary, body,
   and the actions for the target:
   - action ids: `open`, `show`, `open-folder`; plus `default` (body click) on Linux.
   - Windows: `.app_id("io.github.opendownloader")`. Linux: `.appname("OpenDownloader")` and
     `.icon("folder-download")` as today.
2. `.show()`, then `handle.wait_for_response(...)`.
   **Not** `wait_for_action`: on Windows that reports a body click as `"__closed"`.
3. A pure function `action_for(response, &target) -> Option<Action>` maps
   `Default` / `Action(id)` / `Closed` to `Open(path)`, `ShowInFolder(path)` or nothing. The thread
   runs it through `open_path` / `show_in_folder`.
4. Targets with no actions (`None`) skip step 2 and the thread ends right after `show()`.

### Linux: bounding waiting threads

GNOME and KDE keep notifications that have actions in their tray, so `wait_for_response` can block for
a long time and each waiting thread holds a D-Bus connection. A global `AtomicUsize` counts waiting
notifications; at **8** or more, new notifications are shown without actions (the same as today) and
don't wait. The counter is decremented when the wait returns.

On Windows the wait returns when the popup times out (about 7 s), so the cap never matters there.
The same code runs on both platforms anyway.

## Windows registration

`register_notifications()` is called once from `main` before `run_native`:

- Writes the 256 px app icon (`icon::rgba(256)`, encoded with the `image` crate already in use) to
  `<data dir>/notification-icon.png` if it's missing.
- Creates/updates `HKCU\Software\Classes\AppUserModelId\io.github.opendownloader` with
  `DisplayName = "OpenDownloader"` and `IconUri = <that png path>`, using `windows-sys` 0.61
  (already in the lockfile; features `Win32_Foundation`, `Win32_System_Registry`).
- Idempotent: it rewrites the values on every start, which also fixes a moved data folder.
- Failures are logged at `debug` and never stop startup. Toasts may then not appear, the same as today.

Installer (`packaging/windows/opendownloader.iss`) gets a `[Registry]` entry with
`Flags: uninsdeletekey dontcreatekey` for that key, so an uninstall removes it. The portable zip
leaves only that small key behind.

## Dependencies

- `notify-rust = "4"` moves from the Linux-only table to the shared `[dependencies]`.
- New: `[target.'cfg(windows)'.dependencies] windows-sys = { version = "0.61", features = ["Win32_Foundation", "Win32_System_Registry"] }`.

## Known limits

- On Windows, buttons and clicks work only while the popup is on screen. After the toast has moved
  to the notification center, clicking it does nothing. Fixing that needs a COM activator, which
  is out of scope.
- That a registry-only AUMID shows the right name and icon without a shortcut must be confirmed on a
  real Windows 11 machine **first**, before building the rest on it.
- Clicks don't bring the OpenDownloader window to the front; they only open the file/folder.

## Testing

- Unit tests for `action_for` (every target × response combination) and for the
  "too many waiting" cap logic.
- `cargo build` + `cargo clippy` on Windows and in WSL (`cargo +1.95`).
- `docs/TESTING.md`: replace "Known gap: desktop notifications are Linux-only" with Windows checks
  (test notification shows with OpenDownloader name and icon, from the installer and from the portable
  zip; Open / Show in folder / body click; uninstall removes the registry key) and add Linux action
  checks on GNOME and KDE.
- The user runs the GUI checks; Claude does not drive the desktop.
