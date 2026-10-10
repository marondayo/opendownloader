//! Per-site sign-in: browser cookies, cookies.txt files, netrc credentials and the
//! native Bilibili QR-code login.

use crate::settings::{AccountConfig, AuthMethod};
use crate::site::Site;
use anyhow::{anyhow, bail, Context};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Browser-like user agent for the site APIs we call directly.
pub const BROWSER_UA: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";

/// yt-dlp arguments that apply a site's sign-in method to one run.
///
/// yt-dlp writes cookies back to the `--cookies` file when it exits, so each run gets a
/// private copy. That keeps parallel downloads from clobbering each other (and the user's
/// own cookies.txt). The copy is deleted when this value is dropped.
#[derive(Debug, Default)]
pub struct AuthArgs {
    pub args: Vec<String>,
    temp_cookie: Option<PathBuf>,
}

impl Drop for AuthArgs {
    fn drop(&mut self) {
        if let Some(p) = self.temp_cookie.take() {
            let _ = std::fs::remove_file(p);
        }
    }
}

impl AuthArgs {
    pub fn prepare(site: Site, account: &AccountConfig, run_tag: &str) -> anyhow::Result<AuthArgs> {
        let paths = crate::paths::get();
        let cookie_source = match account.method {
            AuthMethod::None => return Ok(AuthArgs::default()),
            AuthMethod::Browser => {
                let mut spec = account.browser.ytdlp_name().to_string();
                if !account.browser_profile.trim().is_empty() {
                    spec.push(':');
                    spec.push_str(account.browser_profile.trim());
                }
                return Ok(AuthArgs { args: vec!["--cookies-from-browser".into(), spec], temp_cookie: None });
            }
            AuthMethod::Password => {
                return Ok(AuthArgs {
                    args: vec![
                        "--netrc".into(),
                        "--netrc-location".into(),
                        paths.netrc_file().to_string_lossy().into_owned(),
                    ],
                    temp_cookie: None,
                });
            }
            AuthMethod::CookiesFile => account
                .cookies_file
                .clone()
                .ok_or_else(|| anyhow!("{}: no cookies.txt file selected (Settings → Accounts)", site.name()))?,
            AuthMethod::QrCode => managed_cookie_file(site),
        };
        if !cookie_source.exists() {
            bail!(
                "{}: cookie file {} is missing; sign in again in Settings → Accounts",
                site.name(),
                cookie_source.display()
            );
        }
        let tmp_dir = paths.temp_dir();
        std::fs::create_dir_all(&tmp_dir)?;
        let tmp = tmp_dir.join(format!("cookies-{}-{run_tag}.txt", site.key()));
        std::fs::copy(&cookie_source, &tmp).with_context(|| format!("copying {}", cookie_source.display()))?;
        restrict_permissions(&tmp);
        Ok(AuthArgs {
            args: vec!["--cookies".into(), tmp.to_string_lossy().into_owned()],
            temp_cookie: Some(tmp),
        })
    }
}

/// Cookie file managed by OpenDownloader (QR code login).
pub fn managed_cookie_file(site: Site) -> PathBuf {
    crate::paths::get().cookies_dir().join(format!("{}.txt", site.key()))
}

/// Makes a file readable by the owner only (secrets live in it).
pub fn restrict_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

// ---------------------------------------------------------------------------
// Cookies

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cookie {
    pub domain: String,
    pub path: String,
    pub secure: bool,
    /// Unix timestamp, 0 = session cookie.
    pub expires: i64,
    pub name: String,
    pub value: String,
}

/// Serializes cookies in the Netscape cookies.txt format yt-dlp understands.
pub fn to_netscape(cookies: &[Cookie]) -> String {
    let mut out = String::from("# Netscape HTTP Cookie File\n# Written by OpenDownloader\n\n");
    for c in cookies {
        let include_sub = if c.domain.starts_with('.') { "TRUE" } else { "FALSE" };
        let secure = if c.secure { "TRUE" } else { "FALSE" };
        out.push_str(&format!(
            "{}\t{include_sub}\t{}\t{secure}\t{}\t{}\t{}\n",
            c.domain, c.path, c.expires, c.name, c.value
        ));
    }
    out
}

/// Parses a Netscape cookies.txt file (also accepts `#HttpOnly_` prefixed lines).
pub fn parse_netscape(text: &str) -> Vec<Cookie> {
    text.lines()
        .filter_map(|line| {
            let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
            if line.starts_with('#') || line.trim().is_empty() {
                return None;
            }
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 7 {
                return None;
            }
            Some(Cookie {
                domain: f[0].to_string(),
                path: f[2].to_string(),
                secure: f[3].eq_ignore_ascii_case("TRUE"),
                expires: f[4].parse().unwrap_or(0),
                name: f[5].to_string(),
                value: f[6..].join("\t"),
            })
        })
        .collect()
}

/// Cookie that indicates a signed-in session for each site.
pub fn login_cookie_names(site: Site) -> &'static [&'static str] {
    match site {
        Site::YouTube => &["LOGIN_INFO", "__Secure-3PSID", "SAPISID"],
        Site::Niconico => &["user_session"],
        Site::Bilibili => &["SESSDATA"],
        Site::SoundCloud => &["oauth_token"],
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookieCheck {
    pub total: usize,
    pub site_cookies: usize,
    pub has_login: bool,
    pub expired_login: bool,
}

/// Inspects a cookies.txt file for a usable login session for `site`.
pub fn check_cookie_file(path: &Path, site: Site) -> anyhow::Result<CookieCheck> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let cookies = parse_netscape(&text);
    let domains: &[&str] = match site {
        Site::YouTube => &["youtube.com", "google.com"],
        Site::Niconico => &["nicovideo.jp"],
        Site::Bilibili => &["bilibili.com"],
        Site::SoundCloud => &["soundcloud.com"],
    };
    let now = chrono::Utc::now().timestamp();
    let for_site: Vec<&Cookie> =
        cookies.iter().filter(|c| domains.iter().any(|d| c.domain.trim_start_matches('.').ends_with(d))).collect();
    let login: Vec<&&Cookie> =
        for_site.iter().filter(|c| login_cookie_names(site).contains(&c.name.as_str())).collect();
    let valid = login.iter().any(|c| c.expires == 0 || c.expires > now);
    Ok(CookieCheck {
        total: cookies.len(),
        site_cookies: for_site.len(),
        has_login: valid,
        expired_login: !login.is_empty() && !valid,
    })
}

// ---------------------------------------------------------------------------
// netrc (credentials for yt-dlp's --netrc)

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetrcEntry {
    pub machine: String,
    pub login: String,
    pub password: String,
}

fn netrc_token(s: &str) -> String {
    if !s.is_empty() && !s.chars().any(|c| c.is_whitespace() || c == '"' || c == '\\' || c == '#') {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn tokenize_netrc(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '"' {
            chars.next();
            let mut tok = String::new();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => {
                        if let Some(n) = chars.next() {
                            tok.push(n)
                        }
                    }
                    '"' => break,
                    c => tok.push(c),
                }
            }
            tokens.push(tok);
        } else {
            let mut tok = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                tok.push(c);
                chars.next();
            }
            tokens.push(tok);
        }
    }
    tokens
}

pub fn parse_netrc(text: &str) -> Vec<NetrcEntry> {
    let tokens = tokenize_netrc(text);
    let mut entries = Vec::new();
    let mut i = 0;
    let mut current: Option<NetrcEntry> = None;
    while i < tokens.len() {
        match tokens[i].as_str() {
            "machine" if i + 1 < tokens.len() => {
                if let Some(e) = current.take() {
                    entries.push(e);
                }
                current = Some(NetrcEntry { machine: tokens[i + 1].clone(), login: String::new(), password: String::new() });
                i += 2;
            }
            "login" if i + 1 < tokens.len() => {
                if let Some(e) = current.as_mut() {
                    e.login = tokens[i + 1].clone();
                }
                i += 2;
            }
            "password" if i + 1 < tokens.len() => {
                if let Some(e) = current.as_mut() {
                    e.password = tokens[i + 1].clone();
                }
                i += 2;
            }
            _ => i += 1,
        }
    }
    if let Some(e) = current {
        entries.push(e);
    }
    entries
}

pub fn format_netrc(entries: &[NetrcEntry]) -> String {
    entries
        .iter()
        .map(|e| {
            format!(
                "machine {} login {} password {}\n",
                netrc_token(&e.machine),
                netrc_token(&e.login),
                netrc_token(&e.password)
            )
        })
        .collect()
}

fn read_netrc() -> Vec<NetrcEntry> {
    std::fs::read_to_string(crate::paths::get().netrc_file()).map(|t| parse_netrc(&t)).unwrap_or_default()
}

fn write_netrc(entries: &[NetrcEntry]) -> anyhow::Result<()> {
    let path = crate::paths::get().netrc_file();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Create with restricted permissions before writing any secret.
    std::fs::write(&path, "")?;
    restrict_permissions(&path);
    std::fs::write(&path, format_netrc(entries))?;
    Ok(())
}

/// Stores credentials for a site. For SoundCloud, pass login `oauth` and the token as password.
pub fn set_credentials(site: Site, login: &str, password: &str) -> anyhow::Result<()> {
    let mut entries: Vec<NetrcEntry> =
        read_netrc().into_iter().filter(|e| e.machine != site.netrc_machine()).collect();
    entries.push(NetrcEntry {
        machine: site.netrc_machine().into(),
        login: login.into(),
        password: password.into(),
    });
    write_netrc(&entries)
}

pub fn remove_credentials(site: Site) -> anyhow::Result<()> {
    let entries: Vec<NetrcEntry> =
        read_netrc().into_iter().filter(|e| e.machine != site.netrc_machine()).collect();
    write_netrc(&entries)
}

pub fn has_credentials(site: Site) -> bool {
    read_netrc().iter().any(|e| e.machine == site.netrc_machine() && !e.password.is_empty())
}

/// Removes everything stored for a site's sign-in (credentials and managed cookies).
pub fn sign_out(site: Site) -> anyhow::Result<()> {
    if has_credentials(site) {
        remove_credentials(site)?;
    }
    let managed = managed_cookie_file(site);
    if managed.exists() {
        std::fs::remove_file(managed)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Bilibili QR code login

pub mod bilibili {
    use super::*;

    const GENERATE: &str = "https://passport.bilibili.com/x/passport-login/web/qrcode/generate";
    const POLL: &str = "https://passport.bilibili.com/x/passport-login/web/qrcode/poll";
    const NAV: &str = "https://api.bilibili.com/x/web-interface/nav";

    #[derive(Debug, Clone)]
    pub struct QrLogin {
        /// Content to encode in the QR code.
        pub url: String,
        pub key: String,
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum PollState {
        Waiting,
        Scanned,
        Expired,
        Success { cookies: Vec<Cookie> },
    }

    #[derive(Deserialize)]
    struct Envelope<T> {
        code: i64,
        #[serde(default)]
        message: String,
        data: Option<T>,
    }

    #[derive(Deserialize)]
    struct GenerateData {
        url: String,
        qrcode_key: String,
    }

    #[derive(Deserialize)]
    struct PollData {
        #[serde(default)]
        url: String,
        code: i64,
    }

    #[derive(Deserialize)]
    struct NavData {
        #[serde(rename = "isLogin", default)]
        is_login: bool,
        #[serde(default)]
        uname: String,
    }

    pub fn client() -> anyhow::Result<reqwest::Client> {
        Ok(reqwest::Client::builder()
            .user_agent(BROWSER_UA)
            .timeout(std::time::Duration::from_secs(20))
            .build()?)
    }

    pub async fn generate(client: &reqwest::Client) -> anyhow::Result<QrLogin> {
        let env: Envelope<GenerateData> = client
            .get(GENERATE)
            .header("Referer", "https://www.bilibili.com/")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let data = env.data.ok_or_else(|| anyhow!("Bilibili: {} ({})", env.message, env.code))?;
        Ok(QrLogin { url: data.url, key: data.qrcode_key })
    }

    pub async fn poll(client: &reqwest::Client, key: &str) -> anyhow::Result<PollState> {
        let resp = client
            .get(POLL)
            .query(&[("qrcode_key", key)])
            .header("Referer", "https://www.bilibili.com/")
            .send()
            .await?
            .error_for_status()?;
        let set_cookies: Vec<String> = resp
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok().map(str::to_string))
            .collect();
        let env: Envelope<PollData> = resp.json().await?;
        let data = env.data.ok_or_else(|| anyhow!("Bilibili: {} ({})", env.message, env.code))?;
        Ok(match data.code {
            0 => {
                let mut cookies: Vec<Cookie> = set_cookies.iter().filter_map(|h| parse_set_cookie(h)).collect();
                if !cookies.iter().any(|c| c.name == "SESSDATA") {
                    cookies = cookies_from_redirect_url(&data.url);
                }
                if !cookies.iter().any(|c| c.name == "SESSDATA") {
                    bail!("Bilibili accepted the login but sent no session cookie");
                }
                PollState::Success { cookies }
            }
            86090 => PollState::Scanned,
            86038 => PollState::Expired,
            _ => PollState::Waiting,
        })
    }

    /// Saves login cookies to the managed cookie file used by the QR code method.
    pub fn save_cookies(cookies: &[Cookie]) -> anyhow::Result<PathBuf> {
        let path = managed_cookie_file(Site::Bilibili);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, "")?;
        restrict_permissions(&path);
        std::fs::write(&path, to_netscape(cookies))?;
        Ok(path)
    }

    /// Returns the nickname of the account in the cookie file, or `None` if signed out.
    pub async fn account_name(client: &reqwest::Client, cookie_file: &Path) -> anyhow::Result<Option<String>> {
        let text = std::fs::read_to_string(cookie_file)?;
        let header = parse_netscape(&text)
            .iter()
            .filter(|c| c.domain.trim_start_matches('.').ends_with("bilibili.com"))
            .map(|c| format!("{}={}", c.name, c.value))
            .collect::<Vec<_>>()
            .join("; ");
        let env: Envelope<NavData> = client
            .get(NAV)
            .header(reqwest::header::COOKIE, header)
            .header("Referer", "https://www.bilibili.com/")
            .send()
            .await?
            .json()
            .await?;
        Ok(env.data.filter(|d| d.is_login).map(|d| d.uname))
    }

    pub(crate) fn parse_set_cookie(header: &str) -> Option<Cookie> {
        let mut parts = header.split(';');
        let (name, value) = parts.next()?.trim().split_once('=')?;
        let mut cookie = Cookie {
            domain: ".bilibili.com".into(),
            path: "/".into(),
            secure: false,
            expires: 0,
            name: name.trim().into(),
            value: value.trim().into(),
        };
        for attr in parts {
            let (k, v) = attr.trim().split_once('=').unwrap_or((attr.trim(), ""));
            match k.to_ascii_lowercase().as_str() {
                "domain" => {
                    let d = v.trim();
                    cookie.domain = if d.starts_with('.') { d.into() } else { format!(".{d}") };
                }
                "path" => cookie.path = v.trim().into(),
                "secure" => cookie.secure = true,
                "expires" => {
                    if let Ok(t) = chrono::DateTime::parse_from_rfc2822(v.trim().replace('-', " ").as_str()) {
                        cookie.expires = t.timestamp();
                    }
                }
                "max-age" => {
                    if let Ok(secs) = v.trim().parse::<i64>() {
                        cookie.expires = chrono::Utc::now().timestamp() + secs;
                    }
                }
                _ => {}
            }
        }
        if cookie.expires == 0 {
            cookie.expires = chrono::Utc::now().timestamp() + 180 * 24 * 3600;
        }
        Some(cookie)
    }

    pub(crate) fn cookies_from_redirect_url(url: &str) -> Vec<Cookie> {
        let Ok(parsed) = url::Url::parse(url) else {
            return Vec::new();
        };
        let pairs: Vec<(String, String)> = parsed.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
        let expires = pairs
            .iter()
            .find(|(k, _)| k == "Expires")
            .and_then(|(_, v)| v.parse::<i64>().ok())
            .unwrap_or_else(|| chrono::Utc::now().timestamp() + 180 * 24 * 3600);
        pairs
            .into_iter()
            .filter(|(k, _)| matches!(k.as_str(), "DedeUserID" | "DedeUserID__ckMd5" | "SESSDATA" | "bili_jct"))
            .map(|(name, value)| Cookie {
                domain: ".bilibili.com".into(),
                path: "/".into(),
                secure: false,
                expires,
                name,
                value,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netscape_roundtrip() {
        let cookies = vec![Cookie {
            domain: ".bilibili.com".into(),
            path: "/".into(),
            secure: true,
            expires: 1_900_000_000,
            name: "SESSDATA".into(),
            value: "abc%2C123".into(),
        }];
        let text = to_netscape(&cookies);
        assert_eq!(parse_netscape(&text), cookies);
        let http_only = "#HttpOnly_.nicovideo.jp\tTRUE\t/\tTRUE\t0\tuser_session\tuser_session_1\n";
        assert_eq!(parse_netscape(http_only)[0].name, "user_session");
    }

    #[test]
    fn netrc_roundtrip_with_spaces() {
        let entries = vec![
            NetrcEntry { machine: "niconico".into(), login: "me@example.com".into(), password: "pa ss\"w\\d".into() },
            NetrcEntry { machine: "soundcloud".into(), login: "oauth".into(), password: "2-123-abc".into() },
        ];
        let text = format_netrc(&entries);
        assert_eq!(parse_netrc(&text), entries);
    }

    #[test]
    fn bilibili_redirect_cookies() {
        let url = "https://passport.biligame.com/x/passport-login/web/crossDomain?DedeUserID=1&DedeUserID__ckMd5=x&Expires=1900000000&SESSDATA=s%2C1&bili_jct=j&gourl=https%3A%2F%2Fwww.bilibili.com";
        let c = bilibili::cookies_from_redirect_url(url);
        assert_eq!(c.len(), 4);
        let s = c.iter().find(|c| c.name == "SESSDATA").unwrap();
        assert_eq!(s.value, "s,1");
        assert_eq!(s.expires, 1_900_000_000);
    }

    #[test]
    fn bilibili_set_cookie() {
        let c = bilibili::parse_set_cookie(
            "SESSDATA=abc%2C1; Path=/; Domain=bilibili.com; Expires=Mon, 06 Apr 2026 10:00:00 GMT; HttpOnly; Secure",
        )
        .unwrap();
        assert_eq!(c.name, "SESSDATA");
        assert_eq!(c.domain, ".bilibili.com");
        assert!(c.secure);
        assert_eq!(c.expires, 1_775_469_600);
    }

    #[test]
    fn cookie_check() {
        let dir = std::env::temp_dir().join(format!("odl-auth-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.txt");
        std::fs::write(
            &path,
            ".youtube.com\tTRUE\t/\tTRUE\t4102444800\tLOGIN_INFO\tx\n.example.com\tTRUE\t/\tFALSE\t0\ta\tb\n",
        )
        .unwrap();
        let check = check_cookie_file(&path, Site::YouTube).unwrap();
        assert!(check.has_login);
        assert_eq!((check.total, check.site_cookies), (2, 1));
        assert!(!check_cookie_file(&path, Site::Niconico).unwrap().has_login);
        let _ = std::fs::remove_dir_all(dir);
    }
}
