# YouTube Sign-in Fallback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a Chromium-based browser's cookies can't be read on Windows (browser open, or app-bound encryption), downloads continue without signing in and say why. Settings guides the user to the `LockProfileCookieDatabase` flag or a private-window cookies.txt export.

**Architecture:** A new `odl_core::browser_cookies` module checks, before yt-dlp runs, whether the browser's cookie database can be read. `AuthArgs::prepare` uses it to skip `--cookies-from-browser` and record why (`signed_out`). `ytdlp.rs` turns that into notes and error tips. The GUI shows a toast when a probe starts, plus a status line, steps and a Check button in Settings → Accounts.

**Tech Stack:** Rust 1.95, odl-core (tokio, anyhow), odl-gui (egui 0.36).

**Spec:** `docs/superpowers/specs/2026-10-16-youtube-signin-fallback-design.md`

---

## Ground rules

- **Git identity:** before every commit, check that `git config user.name` prints `marondayo`; stop if it doesn't. End every commit message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Windows:** `cargo +1.95 test --workspace`. **WSL:** `wsl -d Ubuntu-24.04 -- bash -lc 'cd /mnt/c/Users/sinta/projects/opendownloader-pro && CARGO_TARGET_DIR=~/odl-target cargo +1.95 test --workspace'`.
- **Clippy** on Windows: `cargo clippy --workspace --all-targets` (stable; 1.95 has no clippy here). Expect no new warnings in touched files.
- After WSL runs, `wsl --terminate Ubuntu-24.04` (it was stopped before).
- **GUI checks are done by the user.** Never drive the desktop.

## Files

| File | Change |
|---|---|
| `crates/odl-core/src/browser_cookies.rs` | create: `Readability`, `check`, `UNLOCK_FLAG`, tests |
| `crates/odl-core/src/lib.rs` | `pub mod browser_cookies;` |
| `crates/odl-core/src/auth.rs` | `AuthArgs::signed_out`, Browser fallback, test |
| `crates/odl-core/src/ytdlp.rs` | `explain_error`, note, call sites, tests |
| `crates/odl-gui/src/app.rs` | `browser_checks` field, toast in `probe` |
| `crates/odl-gui/src/views/settings.rs` | Accounts: status + Check + flag steps, cookies.txt steps, Deno hint |

---

### Task 1: `browser_cookies` module (TDD)

- [ ] **Step 1:** add `pub mod browser_cookies;` to `crates/odl-core/src/lib.rs` (alphabetical, after `pub mod bilicdn;`), and create `crates/odl-core/src/browser_cookies.rs` containing only the tests below plus `use super::*` scaffolding (the module body is empty for now):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("odl-bc-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_db(dir: &Path, age_secs: u64) -> PathBuf {
        let net = dir.join("Network");
        fs::create_dir_all(&net).unwrap();
        let db = net.join("Cookies");
        let f = fs::File::create(&db).unwrap();
        f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(age_secs)).unwrap();
        db
    }

    #[test]
    fn finds_profiles_like_ytdlp() {
        let root = temp("profiles");
        let default = make_db(&root.join("Default"), 3600);
        let p2 = make_db(&root.join("Profile 2"), 10);
        assert_eq!(find_cookie_db(&root, ""), Some(p2.clone()), "empty profile = most recently used");
        assert_eq!(find_cookie_db(&root, "Default"), Some(default));
        assert_eq!(find_cookie_db(&root, root.join("Profile 2").to_str().unwrap()), Some(p2));
        assert_eq!(find_cookie_db(&root, "Missing"), None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn detects_app_bound_encryption() {
        assert!(is_app_bound(r#"{"os_crypt":{"app_bound_encrypted_key":"QVBQ","encrypted_key":"RFBB"}}"#));
        assert!(!is_app_bound(r#"{"os_crypt":{"encrypted_key":"RFBB"}}"#));
        assert!(!is_app_bound(""));
    }

    #[test]
    fn classifies_profiles() {
        let root = temp("classify");
        assert_eq!(classify(&root, ""), Readability::NotFound);
        make_db(&root.join("Default"), 0);
        assert_eq!(classify(&root, ""), Readability::Readable);
        fs::write(root.join("Local State"), r#"{"os_crypt":{"app_bound_encrypted_key":"x"}}"#).unwrap();
        assert_eq!(classify(&root, ""), Readability::AppBound);
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn locked_while_another_handle_is_exclusive() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = temp("locked");
        let db = make_db(&root.join("Default"), 0);
        let held = fs::OpenOptions::new().read(true).share_mode(0).open(&db).unwrap();
        assert_eq!(classify(&root, ""), Readability::Locked);
        drop(held);
        assert_eq!(classify(&root, ""), Readability::Readable);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn problems_name_the_browser() {
        assert!(Readability::Locked.problem(Browser::Vivaldi).unwrap().contains("Vivaldi is open"));
        assert!(Readability::AppBound.problem(Browser::Chrome).unwrap().contains("Google Chrome"));
        assert_eq!(Readability::Readable.problem(Browser::Vivaldi), None);
        assert_eq!(Readability::NotFound.problem(Browser::Vivaldi), None);
    }

    #[test]
    fn other_platforms_and_firefox_are_left_to_ytdlp() {
        assert_eq!(check(Browser::Firefox, ""), Readability::Readable);
    }
}
```

- [ ] **Step 2:** `cargo +1.95 test -p odl-core browser_cookies` → FAIL to compile (`find_cookie_db`, `Readability`, … not found).

- [ ] **Step 3:** put this above the tests:

```rust
//! Whether yt-dlp can read a browser's cookies right now.
//!
//! On Windows, Chromium-based browsers keep their cookie database locked while they run, so
//! `--cookies-from-browser` fails until the browser is closed or started with [`UNLOCK_FLAG`].
//! Chrome and Edge 127+ also use app-bound encryption, which yt-dlp can't decrypt at all.
//! Checking first lets a run continue without signing in instead of failing.

use crate::settings::Browser;
use std::path::{Path, PathBuf};

/// Browser flag that stops Chromium from locking its cookie database.
pub const UNLOCK_FLAG: &str = "--disable-features=LockProfileCookieDatabase";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readability {
    /// Readable, or nothing known to be wrong (yt-dlp gets to try).
    Readable,
    /// The browser is running and keeps its cookie database locked.
    Locked,
    /// App-bound encryption, which yt-dlp can't decrypt.
    AppBound,
    /// No cookie database for this browser and profile.
    NotFound,
}

impl Readability {
    /// Why signing in is skipped, for the states where it is.
    pub fn problem(self, browser: Browser) -> Option<String> {
        let name = browser.label();
        match self {
            Readability::Locked => Some(format!("{name} is open and locks its cookies")),
            Readability::AppBound => Some(format!("{name} encrypts its cookies in a way yt-dlp can't read on Windows")),
            Readability::Readable | Readability::NotFound => None,
        }
    }
}

/// Checks `browser`'s cookies (`profile`: a profile name or path, empty for the most recently used one).
pub fn check(browser: Browser, profile: &str) -> Readability {
    #[cfg(windows)]
    if let Some(dir) = user_data_dir(browser) {
        return classify(&dir, profile);
    }
    let _ = (browser, profile);
    Readability::Readable
}

/// Where a Chromium-based browser keeps its profiles on Windows (the folders yt-dlp searches).
#[cfg(windows)]
fn user_data_dir(browser: Browser) -> Option<PathBuf> {
    let local = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    let roaming = PathBuf::from(std::env::var_os("APPDATA")?);
    Some(match browser {
        Browser::Firefox => return None,
        Browser::Chrome => local.join(r"Google\Chrome\User Data"),
        Browser::Chromium => local.join(r"Chromium\User Data"),
        Browser::Brave => local.join(r"BraveSoftware\Brave-Browser\User Data"),
        Browser::Edge => local.join(r"Microsoft\Edge\User Data"),
        Browser::Vivaldi => local.join(r"Vivaldi\User Data"),
        Browser::Opera => roaming.join(r"Opera Software\Opera Stable"),
    })
}

/// [`check`] for a browser whose profiles live in `user_data`.
fn classify(user_data: &Path, profile: &str) -> Readability {
    let Some(db) = find_cookie_db(user_data, profile) else { return Readability::NotFound };
    let local_state = std::fs::read_to_string(user_data.join("Local State")).unwrap_or_default();
    if is_app_bound(&local_state) {
        Readability::AppBound
    } else if is_locked(&db) {
        Readability::Locked
    } else {
        Readability::Readable
    }
}

/// The cookie database yt-dlp would read: the given profile's, or else the most recently
/// written one in `user_data` or a profile folder below it.
fn find_cookie_db(user_data: &Path, profile: &str) -> Option<PathBuf> {
    let in_dir = |dir: &Path| [dir.join("Network").join("Cookies"), dir.join("Cookies")].into_iter().find(|p| p.is_file());
    let profile = profile.trim();
    if !profile.is_empty() {
        let p = Path::new(profile);
        return in_dir(&if p.is_absolute() { p.to_path_buf() } else { user_data.join(p) });
    }
    let mut dirs = vec![user_data.to_path_buf()];
    if let Ok(entries) = std::fs::read_dir(user_data) {
        dirs.extend(entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
    }
    dirs.iter().filter_map(|d| in_dir(d)).max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok())
}

/// Whether a `Local State` file says cookies use app-bound encryption.
fn is_app_bound(local_state: &str) -> bool {
    local_state.contains("\"app_bound_encrypted_key\"")
}

/// Whether another process holds `path` open exclusively (a Windows sharing violation).
fn is_locked(path: &Path) -> bool {
    const ERROR_SHARING_VIOLATION: i32 = 32;
    match std::fs::File::open(path) {
        Ok(_) => false,
        Err(e) => cfg!(windows) && e.raw_os_error() == Some(ERROR_SHARING_VIOLATION),
    }
}
```

Note: with a profile *path*, `Local State` is looked up in `user_data` (the browser's own folder); a profile path outside it is unusual and then just skips the app-bound check.

- [ ] **Step 4:** `cargo +1.95 test -p odl-core browser_cookies` → all PASS (the Windows-only lock test runs on Windows).

---

### Task 2: Fallback in `AuthArgs`

- [ ] **Step 1 (test):** add to `crates/odl-core/src/auth.rs` tests:

```rust
    #[test]
    fn browser_sign_in_passes_cookies_when_readable() {
        let account = AccountConfig { method: AuthMethod::Browser, browser: Browser::Firefox, ..Default::default() };
        let auth = AuthArgs::prepare(Site::YouTube, &account, "t").unwrap();
        assert_eq!(auth.args, vec!["--cookies-from-browser".to_string(), "firefox".to_string()]);
        assert_eq!(auth.signed_out, None);
    }
```

(import `crate::settings::Browser` in the test module if not in scope).

- [ ] **Step 2:** run it → FAIL (`signed_out` doesn't exist).

- [ ] **Step 3 (impl):** in `AuthArgs` add, after `temp_cookie`:

```rust
    /// Set when sign-in was skipped because the browser's cookies can't be read; says why.
    pub signed_out: Option<String>,
```

add `signed_out: None` to the three existing `AuthArgs { … }` literals, and start the `AuthMethod::Browser` arm with:

```rust
            AuthMethod::Browser => {
                if let Some(reason) =
                    crate::browser_cookies::check(account.browser, &account.browser_profile).problem(account.browser)
                {
                    tracing::info!("{}: continuing without signing in: {reason}", site.name());
                    return Ok(AuthArgs { args: Vec::new(), temp_cookie: None, signed_out: Some(reason) });
                }
```

(the rest of the arm unchanged; `AuthArgs` implements `Drop`, so no `..Default::default()`).

- [ ] **Step 4:** `cargo +1.95 test -p odl-core auth` → PASS.

---

### Task 3: Error tips and the download note (`ytdlp.rs`)

- [ ] **Step 1 (tests):** add next to `friendly_errors`:

```rust
    #[test]
    fn explains_skipped_sign_in_only_when_an_account_is_needed() {
        let members = vec!["ERROR: [youtube] abc: Join this channel to get access to members-only content".to_string()];
        let msg = explain_error(&members, Site::YouTube, Some("Vivaldi is open and locks its cookies"));
        assert!(msg.contains("needs a signed-in account, but Vivaldi is open"), "{msg}");
        assert!(friendly_error(&members, Site::YouTube).contains("Tip: sign in to YouTube"));
        let forbidden = vec!["ERROR: unable to download video data: HTTP Error 403: Forbidden".to_string()];
        assert!(!explain_error(&forbidden, Site::YouTube, Some("x")).contains("signed-in account"));
    }

    #[test]
    fn cookie_read_errors_get_their_own_tips() {
        let locked = vec!["ERROR: Could not copy Chrome cookie database. See  https://github.com/yt-dlp/yt-dlp/issues/7271  for more info".to_string()];
        let msg = friendly_error(&locked, Site::YouTube);
        assert!(msg.contains(crate::browser_cookies::UNLOCK_FLAG) && !msg.contains("Tip: sign in"), "{msg}");
        let dpapi = vec!["ERROR: Failed to decrypt with DPAPI. See https://github.com/yt-dlp/yt-dlp/issues/10927".to_string()];
        assert!(friendly_error(&dpapi, Site::YouTube).contains("Use Firefox or a cookies.txt file"));
        let rotated = vec![
            "WARNING: [youtube] The provided YouTube account cookies are no longer valid. They have likely been rotated in the browser as a security measure.".to_string(),
            "ERROR: [youtube] abc: Sign in to confirm you're not a bot".to_string(),
        ];
        assert!(friendly_error(&rotated, Site::YouTube).contains("Export them again"));
    }
```

- [ ] **Step 2:** run → FAIL (`explain_error` not found).

- [ ] **Step 3 (impl):** replace `friendly_error` with:

```rust
/// Turns yt-dlp's stderr into a short, actionable message.
pub fn friendly_error(stderr_lines: &[String], site: Site) -> String {
    explain_error(stderr_lines, site, None)
}

/// [`friendly_error`] for a run that skipped sign-in (`signed_out` says why): a failure that needs
/// an account then explains the skipped sign-in instead of asking the user to sign in.
pub fn explain_error(stderr_lines: &[String], site: Site, signed_out: Option<&str>) -> String {
    let error = stderr_lines
        .iter()
        .rev()
        .find(|l| l.starts_with("ERROR:"))
        .map(|l| l.trim_start_matches("ERROR:").trim().to_string())
        .or_else(|| stderr_lines.iter().rev().find(|l| !l.trim().is_empty()).cloned())
        .unwrap_or_else(|| "yt-dlp failed without an error message".into());
    let lower = error.to_lowercase();
    let rotated = stderr_lines.iter().any(|l| l.to_lowercase().contains("cookies are no longer valid"));
    let hint = if let Some(reason) = signed_out.filter(|_| needs_account(&lower)) {
        Some(format!("Tip: this needs a signed-in account, but {reason}. Settings → Accounts shows how to fix it."))
    } else if lower.contains("could not copy chrome cookie database") {
        Some(format!(
            "Tip: the browser is open and locks its cookies. Close it completely, or start it with {} (Settings → Accounts).",
            crate::browser_cookies::UNLOCK_FLAG
        ))
    } else if lower.contains("dpapi") || lower.contains("app-bound") {
        Some("Tip: this browser's cookies can't be read on Windows. Use Firefox or a cookies.txt file (Settings → Accounts).".into())
    } else if rotated {
        Some(format!("Tip: your {} cookies have expired. Export them again (Settings → Accounts).", site.name()))
    } else if site == Site::Bilibili && lower.contains("412") {
```

…and keep the rest of the old `if` chain and the final `match hint` exactly as they were. Add below:

```rust
/// Whether a yt-dlp error means the video needs a signed-in account.
fn needs_account(lower: &str) -> bool {
    [
        "sign in",
        "login required",
        "members-only",
        "join this channel",
        "private video",
        "age-restricted",
        "confirm your age",
        "inappropriate for some users",
    ]
    .iter()
    .any(|k| lower.contains(k))
}
```

- [ ] **Step 4 (call sites):**
  - `probe`: replace `drop(auth);` … `return Err(anyhow!(friendly_error(&lines, site)));` so it reads the reason first:

```rust
    let output = cmd.output().await.context("could not start yt-dlp")?;
    let signed_out = auth.signed_out.clone();
    drop(auth);
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let lines: Vec<String> = stderr.lines().map(strip_ansi).collect();
        return Err(anyhow!(explain_error(&lines, site, signed_out.as_deref())));
    }
```

  - `download`: `return Err(JobError::Failed(explain_error(&stderr_lines, job.site, auth.signed_out.as_deref())));`
  - `download`, just before `drop(auth);` near the end: `let signed_out_note = auth.signed_out.as_ref().map(|r| format!("Downloaded without signing in: {r}"));` and in `JobOutcome` use
    `_ => [signed_out_note, comments_note].into_iter().flatten().reduce(|a, b| format!("{a} · {b}")),`

- [ ] **Step 5:** `cargo +1.95 test -p odl-core` → all PASS.
- [ ] **Step 6:** commit Tasks 1-3: `YouTube sign-in: continue without cookies when the browser's can't be read`.

---

### Task 4: GUI

- [ ] **Step 1 (`app.rs`):** next to `cookie_checks`, add `pub browser_checks: HashMap<Site, odl_core::browser_cookies::Readability>,` and `browser_checks: HashMap::new(),` in the constructor. In `pub fn probe`, before `self.rt.spawn`:

```rust
        let acct = self.settings.account(site);
        if acct.method == AuthMethod::Browser {
            if let Some(reason) = odl_core::browser_cookies::check(acct.browser, &acct.browser_profile).problem(acct.browser) {
                self.toast(ToastKind::Info, format!("Not signed in to {}: {reason}. Settings → Accounts shows how to fix it.", site.name()));
            }
        }
```

- [ ] **Step 2 (`settings.rs`):** `use odl_core::browser_cookies::{self, Readability, UNLOCK_FLAG};`. In `account_card`, `AuthMethod::Browser` arm: keep the browser/profile row, then replace the `hint(…)` with:

```rust
                let (browser, profile) = (acct.browser, acct.browser_profile.clone());
                if cfg!(windows) && browser != Browser::Firefox {
                    if changed {
                        self.browser_checks.remove(&site);
                    }
                    let check = *self.browser_checks.entry(site).or_insert_with(|| browser_cookies::check(browser, &profile));
                    let name = browser.label();
                    let (text, color) = match check {
                        Readability::Readable => (format!("✔ {name}'s cookies can be read"), ACCENT_HOVER),
                        Readability::Locked => (format!("✖ {name} is open and locks its cookies. Downloads continue without signing in."), DANGER),
                        Readability::AppBound => (format!("✖ {name} encrypts its cookies in a way yt-dlp can't read on Windows. Use Firefox or a cookies.txt file."), DANGER),
                        Readability::NotFound => (format!("⚠ No {name} profile found"), WARN),
                    };
                    let mut recheck = false;
                    ui.horizontal(|ui| {
                        recheck = ui.button("Check").clicked();
                        ui.add(egui::Label::new(RichText::new(text).color(color)).wrap());
                    });
                    if recheck {
                        self.browser_checks.insert(site, browser_cookies::check(browser, &profile));
                    }
                    if check != Readability::AppBound {
                        hint(ui, &format!(
                            "{name} locks its cookies while it's open. To stay signed in with {name} open: close {name}, \
                             right-click the shortcut you start it from → Properties (taskbar icon: right-click it, then \
                             right-click {name} → Properties), add the flag below to the end of Target after a space, \
                             click OK and start {name} again. Then press Check."
                        ));
                        let mut copied = false;
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(UNLOCK_FLAG).monospace());
                            if ui.small_button("Copy").clicked() {
                                ui.ctx().copy_text(UNLOCK_FLAG.to_string());
                                copied = true;
                            }
                        });
                        if copied {
                            self.toast(ToastKind::Info, "Flag copied");
                        }
                    }
                } else {
                    hint(ui, &format!( /* the existing hint text, unchanged */ ));
                }
```

- [ ] **Step 3:** `AuthMethod::CookiesFile` arm: replace the last `hint(…)` with

```rust
                hint(ui, if site == Site::YouTube {
                    "YouTube rotates the cookies of an open session, so export them from a private window: sign in to \
                     YouTube in a new private window, open youtube.com/robots.txt in that tab, export the youtube.com \
                     cookies with a cookies.txt extension (e.g. “Get cookies.txt LOCALLY”), then close the private \
                     window without signing out."
                } else {
                    "Export a Netscape-format cookies.txt while signed in, e.g. with a “Get cookies.txt LOCALLY” browser extension."
                });
```

- [ ] **Step 4:** after the `match method { … }` in `account_card`:

```rust
        if site == Site::YouTube && method != AuthMethod::None && self.tools.deno.is_none() {
            ui.label(RichText::new("⚠ Signed-in YouTube downloads need Deno. Install it in Settings → Components.").color(WARN));
        }
```

- [ ] **Step 5:** build + test + clippy on Windows and WSL; commit `Settings → Accounts: browser cookie check, unlock-flag steps, private-window export`.

### Task 4b: Deno required, `--remote-components` for yt-dlp from PATH (added at the user's request)

- [ ] Test in `ytdlp.rs`: `site_args` for YouTube with a `System` yt-dlp has `--remote-components ejs:github`; a `Managed` one doesn't; other sites get nothing.
- [ ] `site_args`: append the flag when `ctx.tools.ytdlp` isn't `ToolSource::Managed`.
- [ ] `tools.rs`: Deno's `purpose()` ends in "(required)". `app.rs`: `missing_required_tools()` uses `Tool::ALL`.
- [ ] Remove the Deno hint from Task 4 Step 4.
- [ ] Test on both platforms; commit.

---

### Task 5: Verify

- [ ] Full workspace tests on both platforms (Ground rules).
- [ ] **USER CHECKPOINT:** with Vivaldi open, run `cargo +1.95 run -p odl-gui`:
  1. Settings → Accounts → YouTube shows "✖ Vivaldi is open and locks its cookies…" plus the flag steps (and the Deno warning if Deno isn't installed).
  2. Paste a public YouTube link → toast "Not signed in to YouTube: Vivaldi is open…" → it downloads; the item shows "Downloaded without signing in: …".
  3. Add the flag to the Vivaldi shortcut, restart Vivaldi, press Check → ✔. A members-only or age-restricted video downloads.
