use crate::{
    ports::{IEditCoordinator, IWorkflowLease, IWorkflowRepository},
    supervisor::filesystem,
    Error, OperationId, Result,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Bind local edit sessions to one real directory and the existing workflow leases.
/// Ancestor aliases are canonicalized once; root symlinks are refused. The lease
/// serializes SDK participants using this repository/root only. It never fences
/// arbitrary OS processes, lower-level engine users, or different repositories.
/// Keep both repository and checkpoint storage outside the synchronized project.
pub struct LocalEditCoordinator {
    root: PathBuf,
    identity: (u64, u64),
    key: OperationId,
    repository: Arc<dyn IWorkflowRepository>,
}
impl LocalEditCoordinator {
    /// Observe the initial root identity without changing its files or permissions.
    pub fn new(root: impl AsRef<Path>, repository: Arc<dyn IWorkflowRepository>) -> Result<Self> {
        let metadata = fs::symlink_metadata(root.as_ref())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::InvalidInput(
                "edit root must be a real directory".into(),
            ));
        }
        let root = fs::canonicalize(root)?;
        let directory = filesystem::directory(&root, false)?;
        let metadata = directory.metadata()?;
        let key = OperationId::new(format!(
            "edit-root-{:x}",
            Sha256::digest(root.as_os_str().as_encoded_bytes())
        ))?;
        Ok(Self {
            root,
            identity: (metadata.dev(), metadata.ino()),
            key,
            repository,
        })
    }
}
struct LocalEditLease {
    _directory: File,
    _workflow: Box<dyn IWorkflowLease>,
}
impl IWorkflowLease for LocalEditLease {}
impl IEditCoordinator for LocalEditCoordinator {
    fn root(&self) -> &Path {
        &self.root
    }
    fn acquire(&self) -> Result<Box<dyn IWorkflowLease>> {
        let lease = self.repository.acquire(&self.key)?;
        let directory = filesystem::directory(&self.root, false)?;
        let metadata = directory.metadata()?;
        if (metadata.dev(), metadata.ino()) != self.identity {
            return Err(Error::Conflict(
                "edit destination directory was replaced; open a new session".into(),
            ));
        }
        Ok(Box::new(LocalEditLease {
            _directory: directory,
            _workflow: lease,
        }))
    }
}
