//! Connection directories under `/tmp`, and the sweep that removes the ones
//! a killed process left behind.
//!
//! ```text
//! make:   /tmp/wsdk-ssh-XXXX (0700) ─► lock (held while the connection lives)
//! sweep:  once per process, before the first connection:
//!         each wsdk-ssh-* directory of this account, older than a minute,
//!         whose lock can be taken (its holder is gone) ─► removed
//! ```
//!
//! The lock is an advisory `flock`, which the system releases when its
//! holder ends however it ends, so a lock that can be taken means no live
//! connection owns the directory. A directory made by a build before the
//! lock existed has none: it is removed once nothing answers on its
//! master's control socket.
use crate::Result;
use fs2::FileExt;
use std::{
    fs::File,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Once,
    time::{Duration, SystemTime},
};

const PREFIX: &str = "wsdk-ssh-";
/// A directory younger than this is never swept: its maker may not have
/// taken the lock yet.
const SWEEP_AFTER: Duration = Duration::from_secs(60);

/// A connection's private directory, owned for as long as this lives.
pub(super) struct ConnectionDirectory {
    // Field order: the lock is released only after the directory is gone.
    directory: tempfile::TempDir,
    _lock: File,
}

impl ConnectionDirectory {
    /// Make a new directory under `/tmp`, sweeping stale ones first, once
    /// per process.
    pub(super) fn make() -> Result<Self> {
        static SWEPT: Once = Once::new();
        SWEPT.call_once(|| sweep(Path::new("/tmp"), SWEEP_AFTER));
        Self::make_in(Path::new("/tmp"))
    }

    /// Make a new directory under `parent`. A short parent: the master's
    /// socket must fit a Unix socket address.
    pub(super) fn make_in(parent: &Path) -> Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix(PREFIX)
            .permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .tempdir_in(parent)?;
        let lock = File::create(directory.path().join("lock"))?;
        lock.try_lock_exclusive()?;
        Ok(Self {
            directory,
            _lock: lock,
        })
    }

    pub(super) fn path(&self) -> &Path {
        self.directory.path()
    }
}

/// Remove every connection directory of this account under `parent` that
/// is older than `after` and that no live process owns. Best effort: what
/// cannot be read or removed is left.
pub(super) fn sweep(parent: &Path, after: Duration) {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    // SAFETY: getuid cannot fail and touches no memory.
    let me = unsafe { libc::getuid() };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(PREFIX) {
            continue;
        }
        let path = entry.path();
        // Not followed: a link is never a connection directory.
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let old = metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= after);
        if metadata.is_dir() && metadata.uid() == me && old && abandoned(&path) {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// Whether no live process owns the connection directory `path`.
fn abandoned(path: &Path) -> bool {
    match File::open(path.join("lock")) {
        // Taken here, it is dropped (and released) before the removal.
        Ok(lock) => lock.try_lock_exclusive().is_ok(),
        // Made before there was a lock: owned while its master answers.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let control: PathBuf = path.join("control");
            std::os::unix::net::UnixStream::connect(control).is_err()
        }
        Err(_) => false,
    }
}

#[cfg(test)]
#[path = "directories_tests.rs"]
mod tests;
