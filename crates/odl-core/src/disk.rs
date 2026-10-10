//! Disk space: free-space queries, the keep-free limit and leftover cleanup.

use std::path::{Path, PathBuf};

/// Free bytes available to the user on the volume holding `path` (or its nearest
/// existing ancestor, since download folders are created lazily).
pub fn free_space(path: &Path) -> Option<u64> {
    let mut p = path;
    while !p.exists() {
        p = p.parent()?;
    }
    free_space_impl(p)
}

#[cfg(unix)]
fn free_space_impl(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    Some(st.f_bavail as u64 * st.f_frsize as u64)
}

#[cfg(not(unix))]
fn free_space_impl(_path: &Path) -> Option<u64> {
    // TODO(windows): GetDiskFreeSpaceExW.
    None
}

/// Why a job can't (or can no longer) run.
pub fn shortage_message(free: u64, reserve: u64, needed: Option<u64>, dir: &Path) -> String {
    use crate::model::format_bytes;
    match needed {
        Some(n) => format!(
            "Not enough disk space: this needs about {}, but only {} is free in {} (keeping {} free). \
             Free up space or choose another folder, then Retry.",
            format_bytes(n),
            format_bytes(free),
            dir.display(),
            format_bytes(reserve)
        ),
        None => format!(
            "Stopped: the disk is almost full ({} free in {}, keep-free limit {}). Free up space, then Retry.",
            format_bytes(free),
            dir.display(),
            format_bytes(reserve)
        ),
    }
}

/// Watches free space while a job runs.
pub struct SpaceGuard {
    dir: PathBuf,
    reserve: u64,
    last_check: std::time::Instant,
}

impl SpaceGuard {
    /// `reserve_mib == 0` disables the guard.
    pub fn new(dir: &Path, reserve_mib: u64) -> Option<Self> {
        (reserve_mib > 0).then(|| Self {
            dir: dir.to_path_buf(),
            reserve: reserve_mib * 1024 * 1024,
            last_check: std::time::Instant::now() - std::time::Duration::from_secs(60),
        })
    }

    /// Checks whether `still_needed` more bytes fit while keeping the reserve free.
    pub fn check_needed(&self, still_needed: u64) -> Result<(), String> {
        match free_space(&self.dir) {
            Some(free) if free < still_needed.saturating_add(self.reserve) => {
                Err(shortage_message(free, self.reserve, Some(still_needed), &self.dir))
            }
            _ => Ok(()),
        }
    }

    /// Periodic check (at most every 3 s): fails when free space drops below the reserve.
    pub fn poll(&mut self) -> Result<(), String> {
        if self.last_check.elapsed() < std::time::Duration::from_secs(3) {
            return Ok(());
        }
        self.last_check = std::time::Instant::now();
        match free_space(&self.dir) {
            Some(free) if free < self.reserve => Err(shortage_message(free, self.reserve, None, &self.dir)),
            _ => Ok(()),
        }
    }
}

/// A leftover working folder (`.odl-partial-*`) from a download that is no longer in the list.
#[derive(Debug, Clone, PartialEq)]
pub struct Leftover {
    pub path: PathBuf,
    pub bytes: u64,
}

pub fn dir_size(path: &Path) -> u64 {
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| match e.file_type() {
                    Ok(t) if t.is_dir() => dir_size(&e.path()),
                    Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
                    Err(_) => 0,
                })
                .sum()
        })
        .unwrap_or(0)
}

/// Finds `.odl-partial-<key>` folders directly inside `dirs` whose key is not in `active_keys`.
pub fn find_leftovers(dirs: &[PathBuf], active_keys: &std::collections::HashSet<String>) -> Vec<Leftover> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for dir in dirs {
        if !seen.insert(dir.clone()) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(key) = name.strip_prefix(".odl-partial-") else { continue };
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) && !active_keys.contains(key) {
                out.push(Leftover { bytes: dir_size(&e.path()), path: e.path() });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_space_of_missing_folder_uses_ancestor() {
        let missing = std::env::temp_dir().join("odl-does-not-exist/a/b");
        if cfg!(unix) {
            assert!(free_space(&missing).is_some_and(|f| f > 0));
        }
    }

    #[test]
    fn guard_reports_shortage() {
        let dir = std::env::temp_dir();
        if let Some(g) = SpaceGuard::new(&dir, 1) {
            if cfg!(unix) {
                assert!(g.check_needed(0).is_ok());
                let err = g.check_needed(u64::MAX / 4).unwrap_err();
                assert!(err.starts_with("Not enough disk space"));
            }
        }
        assert!(SpaceGuard::new(&dir, 0).is_none());
    }

    #[test]
    fn leftovers_skip_active_downloads() {
        let base = std::env::temp_dir().join(format!("odl-leftover-test-{}", std::process::id()));
        std::fs::create_dir_all(base.join(".odl-partial-1-100")).unwrap();
        std::fs::create_dir_all(base.join(".odl-partial-2-200")).unwrap();
        std::fs::create_dir_all(base.join("normal-folder")).unwrap();
        std::fs::write(base.join(".odl-partial-2-200/x.part"), vec![0u8; 1000]).unwrap();
        let active = ["1-100".to_string()].into_iter().collect();
        let found = find_leftovers(&[base.clone(), base.clone()], &active);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].bytes, 1000);
        assert!(found[0].path.ends_with(".odl-partial-2-200"));
        let _ = std::fs::remove_dir_all(base);
    }
}
