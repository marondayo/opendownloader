//! Whether yt-dlp can read a browser's cookies right now.
//!
//! On Windows, Chromium-based browsers keep their cookie database locked while they run, so
//! `--cookies-from-browser` fails until the browser is closed. (The old workaround,
//! `--disable-features=LockProfileCookieDatabase`, no longer unlocks it: tested with Vivaldi on
//! Chromium 152.) Chrome and Edge 127+ also use app-bound encryption, which yt-dlp can't decrypt
//! at all. Checking first lets a run continue without signing in instead of failing.

use crate::settings::Browser;
use std::path::{Path, PathBuf};

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
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
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
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
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
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn is_app_bound(local_state: &str) -> bool {
    local_state.contains("\"app_bound_encrypted_key\"")
}

/// Whether another process holds `path` open exclusively (a Windows sharing violation).
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn is_locked(path: &Path) -> bool {
    const ERROR_SHARING_VIOLATION: i32 = 32;
    match std::fs::File::open(path) {
        Ok(_) => false,
        Err(e) => cfg!(windows) && e.raw_os_error() == Some(ERROR_SHARING_VIOLATION),
    }
}

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
