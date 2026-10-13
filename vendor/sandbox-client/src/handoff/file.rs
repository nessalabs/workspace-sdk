#[cfg(unix)]
mod unix {
    use super::super::*;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    use std::{
        fs::{self, File, OpenOptions},
        io::{Read, Write},
        path::{Path, PathBuf},
    };

    /// Local Unix filesystem store, with atomic replacement, fsync and OS locks.
    ///
    /// Use an application-owned directory on a local filesystem that supports
    /// file locking, atomic rename and directory fsync. Do not use this backend
    /// for cross-host coordination or assume those guarantees on network filesystems.
    /// Receipts contain references, not checkpoint data. New directories/files use
    /// 0700/0600 permissions. Existing directory permissions are not changed.
    #[derive(Clone, Debug)]
    pub struct FileReceiptStore {
        directory: PathBuf,
        max_receipt_bytes: usize,
    }
    impl FileReceiptStore {
        /// Select a receipt directory. Files are created lazily on the first write.
        pub fn new(directory: impl AsRef<Path>) -> HandoffResult<Self> {
            let directory = std::path::absolute(directory)?;
            Ok(Self {
                directory,
                max_receipt_bytes: 1024 * 1024,
            })
        }
        /// Limit serialized receipts (default 1 MiB); the same limit applies to reads.
        pub fn with_max_receipt_bytes(mut self, bytes: usize) -> Self {
            self.max_receipt_bytes = bytes;
            self
        }
        fn read(&self, id: &str) -> HandoffResult<Option<HandoffReceipt>> {
            validate_id(id)?;
            let file = match File::open(self.directory.join(format!("{}.json", filename(id)))) {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e.into()),
            };
            let mut bytes = Vec::new();
            file.take((self.max_receipt_bytes as u64).saturating_add(1))
                .read_to_end(&mut bytes)?;
            if bytes.len() > self.max_receipt_bytes {
                return Err(HandoffError::TooLarge);
            }
            let receipt: HandoffReceipt =
                serde_json::from_slice(&bytes).map_err(|_| HandoffError::Corrupt)?;
            receipt.validate()?;
            if receipt.id != id {
                return Err(HandoffError::Corrupt);
            }
            Ok(Some(receipt))
        }
        fn load_committed(&self, id: &str) -> HandoffResult<Option<HandoffReceipt>> {
            validate_id(id)?;
            let lock = match OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(self.directory.join(format!("{}.lock", filename(id))))
            {
                Ok(lock) => lock,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e.into()),
            };
            let _lock = acquire_lock(lock)?;
            let receipt = self.read(id)?;
            // A failed or canceled writer may have renamed the file before its
            // durability acknowledgement. Complete the sync before exposing it.
            if receipt.is_some() {
                sync_ancestors(&self.directory)?;
            }
            Ok(receipt)
        }
        fn write(
            &self,
            receipt: &HandoffReceipt,
            expected_revision: Option<u64>,
        ) -> HandoffResult<()> {
            receipt.validate()?;
            let bytes = serde_json::to_vec(receipt).map_err(|_| HandoffError::Corrupt)?;
            if bytes.len() > self.max_receipt_bytes {
                return Err(HandoffError::TooLarge);
            }
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&self.directory)?;
            // Persist newly created directory entries as well as receipt replacements.
            sync_ancestors(&self.directory)?;
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(
                    self.directory
                        .join(format!("{}.lock", filename(&receipt.id))),
                )?;
            let _lock = acquire_lock(lock)?;
            // Keep the lock file permanently: removing it can split concurrent writers
            // across different inodes. The guard explicitly releases the OS lock.
            let current = self.read(&receipt.id)?;
            if current.as_ref().map(|r| r.revision) != expected_revision {
                return Err(HandoffError::Conflict);
            }
            match &current {
                Some(old)
                    if old.plan != receipt.plan
                        || old.revision.checked_add(1) != Some(receipt.revision) =>
                {
                    return Err(HandoffError::Conflict)
                }
                None if receipt.revision != 0 || receipt.phase != HandoffPhase::Prepared => {
                    return Err(HandoffError::Conflict)
                }
                _ => {}
            }
            let temp_path = self.directory.join(format!(
                ".{}.{:032x}.tmp",
                filename(&receipt.id),
                fastrand::u128(..)
            ));
            let mut temp = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp_path)?;
            let cleanup = TempPath(temp_path);
            temp.write_all(&bytes)?;
            temp.sync_all()?;
            fs::rename(
                &cleanup.0,
                self.directory
                    .join(format!("{}.json", filename(&receipt.id))),
            )?;
            File::open(&self.directory)?.sync_all()?;
            Ok(())
        }
    }
    // Injective on case-insensitive filesystems too. Bounded logical IDs keep
    // receipt, lock, and temporary basenames below common 255-byte limits.
    fn filename(id: &str) -> String {
        id.bytes().map(|byte| format!("{byte:02x}")).collect()
    }
    struct ReceiptLock(File);
    impl Drop for ReceiptLock {
        fn drop(&mut self) {
            // Closing this descriptor alone is insufficient when a concurrent
            // process spawn has inherited the same open file description.
            let _ = self.0.unlock();
        }
    }
    fn acquire_lock(lock: File) -> HandoffResult<ReceiptLock> {
        match lock.try_lock() {
            Ok(()) => Ok(ReceiptLock(lock)),
            Err(std::fs::TryLockError::WouldBlock) => Err(HandoffError::Busy),
            Err(std::fs::TryLockError::Error(e)) => Err(e.into()),
        }
    }
    // Newly created nested directories must also survive a crash. Sync from the
    // leaf up to the root, so no parent can lose a successfully committed child.
    fn sync_ancestors(directory: &Path) -> std::io::Result<()> {
        for ancestor in directory.ancestors() {
            File::open(ancestor)?.sync_all()?;
        }
        Ok(())
    }
    struct TempPath(PathBuf);
    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    impl ReceiptStore for FileReceiptStore {
        async fn load(&self, id: &str) -> HandoffResult<Option<HandoffReceipt>> {
            let store = self.clone();
            let id = id.to_owned();
            tokio::task::spawn_blocking(move || store.load_committed(&id))
                .await
                .map_err(HandoffError::storage)?
        }
        async fn compare_exchange(
            &self,
            receipt: &HandoffReceipt,
            expected_revision: Option<u64>,
        ) -> HandoffResult<()> {
            let store = self.clone();
            let receipt = receipt.clone();
            tokio::task::spawn_blocking(move || store.write(&receipt, expected_revision))
                .await
                .map_err(HandoffError::storage)?
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn lock_release_does_not_wait_for_duplicate_descriptors() {
            let path = std::env::temp_dir().join(format!(
                "boat-client-lock-release-{:032x}",
                fastrand::u128(..)
            ));
            let cleanup = TempPath(path.clone());
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .unwrap();
            // A duplicate shares the open file description, just as a descriptor
            // inherited across fork does before the child's exec closes it.
            let duplicate = file.try_clone().unwrap();
            let guard = acquire_lock(file).unwrap();
            let contender = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .unwrap();
            assert!(matches!(
                contender.try_lock(),
                Err(std::fs::TryLockError::WouldBlock)
            ));
            drop(guard);
            contender
                .try_lock()
                .expect("completed operation must release its lock");
            contender.unlock().unwrap();
            drop(duplicate);
            drop(contender);
            drop(cleanup);
        }
    }
}
#[cfg(unix)]
pub use unix::FileReceiptStore;
