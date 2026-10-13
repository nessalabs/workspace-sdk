use super::{IPreparationLease, IPreparationStore, PreparationReceipt};
use crate::{Error, Result};
use async_trait::async_trait;
use fs2::FileExt;
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
use uuid::Uuid;

/// Atomic controller-local receipts and nonblocking cross-instance file leases.
/// Controllers sharing this directory must acquire a preparation lease before
/// read/side-effect/write operations. Distributed stores must implement the same
/// operation-ownership contract; atomic saves alone do not coordinate effects.
pub struct FilePreparationStore {
    root: PathBuf,
    lock: Mutex<()>,
}

struct FileLease(File);
impl IPreparationLease for FileLease {}
impl Drop for FileLease {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

impl FilePreparationStore {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        Ok(Self {
            root: fs::canonicalize(root.as_ref())?,
            lock: Mutex::new(()),
        })
    }
    fn path(&self, id: &str) -> PathBuf {
        self.root
            .join(format!("{:x}.json", Sha256::digest(id.as_bytes())))
    }
    fn read(&self, id: &str) -> Result<Option<PreparationReceipt>> {
        match fs::read(self.path(id)) {
            Ok(bytes) => {
                let receipt: PreparationReceipt = serde_json::from_slice(&bytes)?;
                if receipt.id != id {
                    return Err(Error::Conflict(
                        "preparation receipt identity does not match its key".into(),
                    ));
                }
                Ok(Some(receipt))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}

#[async_trait]
impl IPreparationStore for FilePreparationStore {
    async fn try_acquire(&self, id: &str) -> Result<Box<dyn IPreparationLease>> {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        options.mode(0o600);
        let file = options.open(self.path(id).with_extension("lock"))?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Box::new(FileLease(file))),
            Err(error) if error.kind() == ErrorKind::WouldBlock => Err(Error::Conflict(
                "preparation is busy; retry this operation later".into(),
            )),
            Err(error) => Err(error.into()),
        }
    }
    async fn load(&self, id: &str) -> Result<Option<PreparationReceipt>> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| Error::State("preparation store lock poisoned".into()))?;
        self.read(id)
    }
    async fn save(&self, receipt: &PreparationReceipt) -> Result<()> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| Error::State("preparation store lock poisoned".into()))?;
        if self.read(&receipt.id)?.is_some_and(|old| {
            old.request_fingerprint != receipt.request_fingerprint
                || old.host_scope != receipt.host_scope
                || old.target_id != receipt.target_id
        }) {
            return Err(Error::Conflict(
                "preparation id was used for a different request".into(),
            ));
        }
        let destination = self.path(&receipt.id);
        let temporary = self.root.join(format!("{}.tmp", Uuid::new_v4()));
        let result = (|| -> Result<()> {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                options.mode(0o600);
            }
            let mut file = options.open(&temporary)?;
            file.write_all(&serde_json::to_vec(receipt)?)?;
            file.sync_all()?;
            fs::rename(&temporary, destination)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}
