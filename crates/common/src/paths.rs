//! Shared paths (socket, config.toml) and the lock against a second start.

use std::fs::{File, TryLockError};
use std::io;
use std::path::{Path, PathBuf};

/// Exclusive lock against a second start; `None` if someone already holds it.
/// The lock holds as long as the returned file is open, and the kernel releases
/// it on exit, even after a crash.
pub fn lock_instance(path: &Path) -> io::Result<Option<File>> {
    let file = File::options()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// Default socket: `$XDG_RUNTIME_DIR/touchveil.sock` (otherwise `/tmp`).
pub fn default_socket() -> PathBuf {
    let dir =
        std::env::var_os("XDG_RUNTIME_DIR").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    dir.join("touchveil.sock")
}

/// Possible locations of config.toml in search order: working directory,
/// `~/.config/touchveil`, `/etc/touchveil`.
pub fn config_candidates() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from(".")];
    let user_base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    if let Some(base) = user_base {
        dirs.push(base.join("touchveil"));
    }
    dirs.push(PathBuf::from("/etc/touchveil"));
    dirs.into_iter().map(|d| d.join("config.toml")).collect()
}

/// First existing config.toml, see [`config_candidates`].
pub fn find_config() -> Option<PathBuf> {
    config_candidates().into_iter().find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_instance_is_detected() {
        let path = std::env::temp_dir().join(format!("touchveil-lock-{}.lock", std::process::id()));
        let first = lock_instance(&path).unwrap();
        assert!(first.is_some());
        assert!(lock_instance(&path).unwrap().is_none());
        drop(first);
        assert!(lock_instance(&path).unwrap().is_some());
        let _ = std::fs::remove_file(path);
    }
}
