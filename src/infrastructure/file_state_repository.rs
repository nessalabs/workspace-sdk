use super::repository_state::RepositoryState;
use crate::{ports::IWorkflowLease, WorkflowRecord};
use crate::{
    ports::IWorkflowRepository, BackendDescriptor, Error, IWorkspaceRepository, OperationId,
    Result, WorkspaceId,
};
use fs2::FileExt;
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

/// Atomic, fsync-backed JSON store with process-level file locking. Keep this
/// application state outside synchronized projects; it contains file references.
pub struct FileStateRepository {
    root: PathBuf,
}
impl FileStateRepository {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        if fs::symlink_metadata(root.as_ref())?
            .file_type()
            .is_symlink()
        {
            return Err(Error::InvalidInput(
                "state directory must not be a symlink".into(),
            ));
        }
        let root = fs::canonicalize(root)?;
        #[cfg(unix)]
        {
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self { root })
    }
    fn access<T>(
        &self,
        write: bool,
        operation: impl FnOnce(&mut RepositoryState) -> Result<T>,
    ) -> Result<T> {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let lock = options.open(self.root.join("state.lock"))?;
        lock.lock_exclusive()?;
        let path = self.root.join("state.json");
        let mut state = if path.exists() {
            let meta = fs::symlink_metadata(&path)?;
            if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 32 * 1024 * 1024 {
                return Err(Error::State("invalid state file".into()));
            }
            let mut state: RepositoryState = serde_json::from_slice(&fs::read(path)?)?;
            if !matches!(state.version, 1 | 2) {
                return Err(Error::Unsupported("state repository version".into()));
            }
            state.version = 2;
            state
        } else {
            RepositoryState::default()
        };
        let result = operation(&mut state)?;
        if write {
            let bytes = serde_json::to_vec_pretty(&state)?;
            if bytes.len() > 32 * 1024 * 1024 {
                return Err(Error::State("state repository exceeds size limit".into()));
            }
            let mut pending = tempfile::NamedTempFile::new_in(&self.root)?;
            pending.write_all(&bytes)?;
            pending.as_file().sync_all()?;
            pending
                .persist(self.root.join("state.json"))
                .map_err(|e| Error::Io(e.error))?;
            File::open(&self.root)?.sync_all()?;
        }
        Ok(result)
    }
}
impl IWorkspaceRepository for FileStateRepository {
    fn reserve(&self, id: &WorkspaceId, identity: &str) -> Result<()> {
        self.access(true, |state| state.reserve(id, identity))
    }
    fn save(&self, descriptor: &BackendDescriptor) -> Result<()> {
        self.access(true, |state| state.save_workspace(descriptor))
    }
    fn get(&self, id: &WorkspaceId) -> Result<Option<BackendDescriptor>> {
        self.access(false, |state| Ok(state.workspaces.get(id).cloned()))
    }
    fn list(&self) -> Result<Vec<BackendDescriptor>> {
        self.access(false, |state| {
            Ok(state.workspaces.values().cloned().collect())
        })
    }
}
impl IWorkflowRepository for FileStateRepository {
    fn acquire(&self, id: &OperationId) -> Result<Box<dyn IWorkflowLease>> {
        let directory = self.root.join("leases");
        fs::create_dir_all(&directory)?;
        if fs::symlink_metadata(&directory)?.file_type().is_symlink() {
            return Err(Error::InvalidInput(
                "workflow lease directory must not be a symlink".into(),
            ));
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let lock = options.open(directory.join(format!("{id}.lock")))?;
        lock.try_lock_exclusive().map_err(|error| {
            if error.kind() == ErrorKind::WouldBlock {
                Error::Conflict(format!(
                    "workflow {id} is already controlled by another operation"
                ))
            } else {
                Error::Io(error)
            }
        })?;
        Ok(Box::new(super::file_workflow_lease::FileWorkflowLease(
            lock,
        )))
    }
    fn load(&self, id: &OperationId) -> Result<Option<WorkflowRecord>> {
        self.access(false, |state| Ok(state.workflows.get(id).cloned()))
    }
    fn save(&self, id: &OperationId, receipt: &WorkflowRecord) -> Result<()> {
        self.access(true, |state| state.save_workflow(id, receipt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reservation_survives_store_reopen_and_rejects_different_request() {
        let temporary = tempfile::tempdir().unwrap();
        let id = WorkspaceId::new("stable").unwrap();
        FileStateRepository::new(temporary.path())
            .unwrap()
            .reserve(&id, "request-one")
            .unwrap();
        let reopened = FileStateRepository::new(temporary.path()).unwrap();
        reopened.reserve(&id, "request-one").unwrap();
        assert!(matches!(
            reopened.reserve(&id, "request-two"),
            Err(Error::Conflict(_))
        ));
    }
    #[test]
    fn corrupted_state_is_not_silently_discarded() {
        let temporary = tempfile::tempdir().unwrap();
        let repository = FileStateRepository::new(temporary.path()).unwrap();
        fs::write(temporary.path().join("state.json"), "invalid").unwrap();
        assert!(repository.list().is_err());
        fs::write(temporary.path().join("state.json"), "{}").unwrap();
        assert!(repository.list().is_err());
    }
    #[test]
    fn lease_excludes_another_repository_instance_until_dropped() {
        let temporary = tempfile::tempdir().unwrap();
        let first = FileStateRepository::new(temporary.path()).unwrap();
        let second = FileStateRepository::new(temporary.path()).unwrap();
        let id = OperationId::new("exclusive").unwrap();
        let lease = first.acquire(&id).unwrap();
        assert!(matches!(second.acquire(&id), Err(Error::Conflict(_))));
        drop(lease);
        assert!(second.acquire(&id).is_ok());
    }
}
