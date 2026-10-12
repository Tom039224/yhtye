//! One app per data directory (`core-design.md` §9, T-65): two apps on the
//! same database would run two cores, each with its own orchestrators, MCP
//! servers and remote server. The app takes an exclusive lock on
//! `<data dir>/yhtye.lock` before it starts the core and holds it until it
//! exits (the OS drops it with the process, also after a crash). A second app
//! on the same directory finds it taken and exits; one with another
//! `YHTYE_DATA_DIR` (and the dev bridge, whose data is elsewhere) is not
//! affected.

use std::fmt;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

/// Name of the lock file in the data directory.
const LOCK_FILE: &str = "yhtye.lock";

/// The held lock; dropping it releases the directory.
#[derive(Debug)]
pub struct DataLock {
    _file: File,
}

#[derive(Debug)]
pub enum LockError {
    /// Another app holds it.
    Taken(PathBuf),
    Io(PathBuf, std::io::Error),
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Taken(dir) => write!(
                f,
                "another Yhtye is already running on {} (set YHTYE_DATA_DIR to run one more on other data)",
                dir.display()
            ),
            Self::Io(path, e) => write!(f, "could not lock {}: {e}", path.display()),
        }
    }
}

impl std::error::Error for LockError {}

/// Locks `dir` for this process (the directory must exist).
pub fn acquire(dir: &Path) -> Result<DataLock, LockError> {
    let path = dir.join(LOCK_FILE);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| LockError::Io(path.clone(), e))?;
    match file.try_lock() {
        Ok(()) => Ok(DataLock { _file: file }),
        Err(TryLockError::WouldBlock) => Err(LockError::Taken(dir.to_path_buf())),
        Err(TryLockError::Error(e)) => Err(LockError::Io(path, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_is_locked_once_until_the_lock_is_dropped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = acquire(dir.path()).expect("the first app locks it");
        assert!(matches!(acquire(dir.path()), Err(LockError::Taken(_))));
        // Another directory is independent.
        let other = tempfile::tempdir().expect("tempdir");
        assert!(acquire(other.path()).is_ok());
        drop(first);
        assert!(acquire(dir.path()).is_ok(), "released when the app exits");
    }
}
