//! Child process helpers: spawning without console windows and killing whole process trees.

use std::ffi::OsStr;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};

/// Creates a command with piped output, null stdin, its own process group (Unix) and
/// no console window (Windows).
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1");
    #[cfg(unix)]
    cmd.process_group(0);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Terminates a child and everything it spawned (yt-dlp starts ffmpeg, for example).
pub async fn kill_tree(child: &mut Child) {
    let Some(pid) = child.id() else {
        return;
    };
    #[cfg(unix)]
    {
        // The child leads its own process group, so signal the whole group.
        let pgid = -(pid as libc::pid_t);
        unsafe {
            libc::kill(pgid, libc::SIGTERM);
        }
        if tokio::time::timeout(Duration::from_secs(3), child.wait()).await.is_err() {
            unsafe {
                libc::kill(pgid, libc::SIGKILL);
            }
            let _ = child.wait().await;
        }
    }
    #[cfg(windows)]
    {
        let _ = command("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output()
            .await;
        let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
        let _ = child.kill().await;
    }
}

/// Reads a stream line by line, tolerating invalid UTF-8 and `\r` separators.
pub struct LineReader<R> {
    inner: BufReader<R>,
    buf: Vec<u8>,
}

impl<R: AsyncRead + Unpin> LineReader<R> {
    pub fn new(inner: R) -> Self {
        Self { inner: BufReader::new(inner), buf: Vec::with_capacity(512) }
    }

    pub async fn next_line(&mut self) -> std::io::Result<Option<String>> {
        loop {
            self.buf.clear();
            let n = self.inner.read_until(b'\n', &mut self.buf).await?;
            if n == 0 {
                return Ok(None);
            }
            let text = String::from_utf8_lossy(&self.buf);
            // Keep only the last carriage-return separated segment (progress bars).
            let line = text.trim_end_matches(['\n', '\r']).rsplit('\r').next().unwrap_or("").trim();
            if !line.is_empty() {
                return Ok(Some(line.to_string()));
            }
        }
    }
}

/// Removes ANSI escape sequences.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Searches `PATH` for an executable.
pub fn find_in_path(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    let names: Vec<String> = if cfg!(windows) && !name.ends_with(".exe") {
        vec![format!("{name}.exe"), name.to_string()]
    } else {
        vec![name.to_string()]
    };
    for dir in std::env::split_paths(&path) {
        for n in &names {
            let candidate = dir.join(n);
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn is_executable(path: &std::path::Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi() {
        assert_eq!(strip_ansi("\u{1b}[0;31mERROR:\u{1b}[0m boom"), "ERROR: boom");
    }

    #[tokio::test]
    async fn line_reader_handles_cr_and_invalid_utf8() {
        let data: &[u8] = b"a\r\n\r\nprog 1\rprog 2\n\xffok\n";
        let mut r = LineReader::new(data);
        assert_eq!(r.next_line().await.unwrap().as_deref(), Some("a"));
        assert_eq!(r.next_line().await.unwrap().as_deref(), Some("prog 2"));
        assert_eq!(r.next_line().await.unwrap().as_deref(), Some("\u{fffd}ok"));
        assert_eq!(r.next_line().await.unwrap(), None);
    }
}
