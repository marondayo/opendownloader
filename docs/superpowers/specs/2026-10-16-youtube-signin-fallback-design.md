# YouTube sign-in with a Chromium browser open (Windows)

Date: 2026-10-16 · Branch: `youtube-signin-fallback`

## Problem

With **Cookies from browser** set to a Chromium-based browser (Vivaldi, Chrome, Edge, Brave, Opera,
Chromium) on Windows, every yt-dlp run fails while that browser is open:

```
ERROR: Could not copy Chrome cookie database. See https://github.com/yt-dlp/yt-dlp/issues/7271
```

The browser opens its `Network\Cookies` SQLite file exclusively, so yt-dlp can't copy it. The app then
shows "Couldn't open this link" with the tip "sign in to YouTube in Settings → Accounts". That tip is
misleading because the user *is* signed in. This has been the case since 0.1.0.

Verified on the reporter's machine (2026-10-16):
- Vivaldi 152 is open, and `Network\Cookies` fails to open with Windows error 32 (sharing violation).
- Vivaldi's `Local State` has `encrypted_key` (DPAPI) and **no** `app_bound_encrypted_key`, so yt-dlp
  can decrypt its cookies whenever the file is readable. Chrome/Edge 127+ use app-bound encryption,
  which yt-dlp can't decrypt on Windows even when the browser is closed.
- Without cookies, the same video downloads fine (up to 2160p). A 403 seen on an earlier attempt was not
  reproduced. Best explanation: signed-in sessions make yt-dlp use clients that need a JavaScript
  runtime, and this setup has no Deno. **Not confirmed.**

The user needs sign-in for members-only and age-restricted videos.

## Goals

1. Public videos always work, even when the browser's cookies can't be read: the run continues
   **without signing in** and says so.
2. A way to stay signed in with Vivaldi open: the `--disable-features=LockProfileCookieDatabase` browser
   flag (option A), with in-app steps and a **Check** button that confirms it worked.
3. A guided cookies.txt export that doesn't go stale quickly (option C).
4. Correct tips for cookie errors, and a hint to install Deno for signed-in YouTube.

Out of scope: unlocking the database from the app (ChromeCookieUnlock / Restart Manager), editing the
user's shortcuts, decrypting app-bound cookies, an in-app Google login.

## Design

### `odl-core/src/browser_cookies.rs` (new)

```rust
pub const UNLOCK_FLAG: &str = "--disable-features=LockProfileCookieDatabase";
pub enum Readability { Readable, Locked, AppBound, NotFound }
impl Readability { pub fn problem(self, browser: Browser) -> Option<String> }   // Some for Locked/AppBound
pub fn check(browser: Browser, profile: &str) -> Readability
```

- `check` only does real work on **Windows for Chromium-based browsers**. Firefox, and every browser
  on other platforms, returns `Readable`, so behaviour there is unchanged.
- Profile folders, the same ones yt-dlp uses:

  | Browser | Folder |
  |---|---|
  | Chrome | `%LOCALAPPDATA%\Google\Chrome\User Data` |
  | Chromium | `%LOCALAPPDATA%\Chromium\User Data` |
  | Brave | `%LOCALAPPDATA%\BraveSoftware\Brave-Browser\User Data` |
  | Edge | `%LOCALAPPDATA%\Microsoft\Edge\User Data` |
  | Vivaldi | `%LOCALAPPDATA%\Vivaldi\User Data` |
  | Opera | `%APPDATA%\Opera Software\Opera Stable` |

- Cookie database: `<profile>\Network\Cookies`, else `<profile>\Cookies`. The profile is the configured
  name (relative to the folder) or an absolute path. If it's empty, the app takes the most recently
  modified database in the folder or one level below it, which is how yt-dlp picks.
- Order of checks:
  1. **NotFound** if there's no database. In that case the app doesn't guess, and yt-dlp tries as before.
  2. **AppBound** if `Local State` contains `"app_bound_encrypted_key"`.
  3. **Locked** if opening the database fails with Windows error 32.
  4. Otherwise **Readable**.

### Fallback: `AuthArgs`

- `AuthArgs` gets `pub signed_out: Option<String>`.
- For `AuthMethod::Browser`, `prepare` calls `check`. If `problem()` returns a reason, it returns no
  yt-dlp arguments and `signed_out = Some(reason)`, and logs it at info level.
- Every run goes through `prepare` (probe, download, Bilibili info, comments), so they all get the
  fallback at the cost of a few file operations, with no second yt-dlp run.
- **Race:** the browser can start between the check and yt-dlp's copy. That run then fails with
  yt-dlp's own error, which now gets the right tip (below).

### Errors and notes (`ytdlp.rs`)

- `friendly_error(lines, site)` stays and becomes `explain_error(lines, site, None)`.
- New `explain_error(lines, site, signed_out: Option<&str>)`. It picks tips in this order:
  1. If sign-in was skipped and the error needs an account ("sign in", "login required", "members-only",
     "join this channel", "private video", "age-restricted", "confirm your age", "inappropriate for some
     users"): *"Tip: this needs a signed-in account, but {reason}. Settings → Accounts shows how to fix it."*
  2. `could not copy chrome cookie database`: close the browser or start it with `UNLOCK_FLAG`.
  3. `dpapi` / `app-bound`: this browser's cookies can't be read on Windows; use Firefox or cookies.txt.
  4. Any stderr line containing `cookies are no longer valid`: the cookies expired; export them again.
  5. Otherwise the existing tips, unchanged.
- `probe` and `download` call `explain_error` with `auth.signed_out`.
- A successful download made without sign-in gets the note **"Downloaded without signing in: {reason}"**,
  joined to any other note with " · ".

### GUI

- **Probe start** (`OdlApp::probe`): if the site uses browser cookies and `check` reports a problem,
  show an Info toast: *"Not signed in to {site}: {reason}. Settings → Accounts shows how to fix it."*
- **Settings → Accounts → Cookies from browser**, on Windows with a Chromium browser:
  - A status line, computed when the card is first shown, cached in `OdlApp::browser_checks`, and
    refreshed by **Check** or by any change to the card:
    - ✔ can be read
    - ✖ is open and locks its cookies (downloads continue without signing in)
    - ✖ app-bound encryption (use Firefox or cookies.txt)
    - ⚠ no profile found
  - Unless the browser is app-bound: steps to add the flag to the shortcut's Target, then **Check**.
    The flag is shown in monospace with a **Copy** button.
  - On other platforms, and for Firefox, the existing hint stays.
- **cookies.txt file**, YouTube only: the hint becomes the yt-dlp wiki procedure. Sign in in a new
  private window, open `youtube.com/robots.txt` in that tab, export the youtube.com cookies, then close
  the private window without signing out.
- **Deno hint:** for YouTube with any sign-in method, if Deno isn't found: *"⚠ Signed-in YouTube downloads
  need Deno. Install it in Settings → Components."*

## Testing

- `browser_cookies`:
  - profile lookup (named, absolute path, newest when empty, missing)
  - app-bound detection from `Local State` text
  - NotFound / AppBound / Readable classification on temp folders
  - Windows only: Locked while the test holds the file open with share mode 0
  - `problem()` texts
- `auth`: browser sign-in on a non-locked setup still passes `--cookies-from-browser`.
- `ytdlp`: the skipped-sign-in tip appears only for errors that need an account; cookie-copy, DPAPI and
  rotated-cookie errors get their own tips; the existing `friendly_errors` test still passes.
- Builds/tests on Windows and in WSL.
- The user's checks in the app:
  1. With Vivaldi open, pasting a public video toasts "Not signed in…" and the download finishes with
     the note.
  2. After adding the flag and restarting Vivaldi, **Check** shows ✔ and a members-only or age-restricted
     video downloads.
