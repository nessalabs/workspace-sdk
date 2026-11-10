use super::repository_state::{RepositoryState, VERSION};
use crate::{
    domain::RecoverySession,
    ports::{IRecoveryRepository, IWorkflowLease},
    WorkflowRecord,
};
use crate::{
    domain::{MachineRecord, MachineState},
    ports::IWorkflowRepository,
    BackendDescriptor, Error, IWorkspaceRepository, OperationId, Result, WorkspaceId,
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
        // Inspect the entry itself: exists() follows dangling links and hides
        // metadata errors. Only a genuinely missing state file starts empty.
        let mut state = match fs::symlink_metadata(&path) {
            Ok(meta) => {
                if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 32 * 1024 * 1024
                {
                    return Err(Error::State("invalid state file".into()));
                }
                let mut state: RepositoryState = serde_json::from_slice(&fs::read(path)?)?;
                if !(1..=VERSION).contains(&state.version) {
                    return Err(Error::Unsupported("state repository version".into()));
                }
                state.version = VERSION;
                state
            }
            Err(error) if error.kind() == ErrorKind::NotFound => RepositoryState::default(),
            Err(error) => return Err(error.into()),
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
    fn begin(
        &self,
        id: &WorkspaceId,
        request: &str,
        initial: MachineState,
    ) -> Result<MachineRecord> {
        self.access(true, |state| state.begin(id, request, initial))
    }
    fn advance(
        &self,
        id: &WorkspaceId,
        expected_revision: u64,
        next: MachineState,
    ) -> Result<MachineRecord> {
        self.access(true, |state| state.advance(id, expected_revision, next))
    }
    fn forget(&self, id: &WorkspaceId, expected_revision: u64) -> Result<()> {
        self.access(true, |state| state.forget(id, expected_revision))
    }
    fn save(&self, descriptor: &BackendDescriptor) -> Result<()> {
        self.access(true, |state| state.save_workspace(descriptor))
    }
    fn record(&self, id: &WorkspaceId) -> Result<Option<MachineRecord>> {
        self.access(false, |state| Ok(state.machines.get(id).cloned()))
    }
    fn records(&self) -> Result<Vec<MachineRecord>> {
        self.access(false, |state| {
            Ok(state.machines.values().cloned().collect())
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

impl IRecoveryRepository for FileStateRepository {
    fn acquire_recovery(&self, id: &OperationId) -> Result<Box<dyn IWorkflowLease>> {
        IWorkflowRepository::acquire(self, id)
    }
    fn load_recovery(&self, id: &OperationId) -> Result<Option<RecoverySession>> {
        self.access(false, |state| state.load_recovery(id))
    }
    fn compare_exchange_recovery(
        &self,
        id: &OperationId,
        expected_revision: Option<u64>,
        replacement: &RecoverySession,
    ) -> Result<()> {
        self.access(true, |state| {
            state.compare_exchange_recovery(id, expected_revision, replacement)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ProvisionKey, RequestOrigin};

    /// Reserve an ID the way an attach does, for tests about the file itself.
    trait Reserve {
        fn reserve(&self, id: &WorkspaceId, request: &str) -> Result<()>;
    }
    impl Reserve for FileStateRepository {
        fn reserve(&self, id: &WorkspaceId, request: &str) -> Result<()> {
            self.begin(id, request, MachineState::Attaching).map(drop)
        }
    }
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
    #[cfg(unix)]
    #[test]
    fn dangling_state_symlink_is_rejected_without_replacing_it() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("state");
        let repository = FileStateRepository::new(&root).unwrap();
        let missing_target = temporary.path().join("missing-private-state.json");
        let state = root.join("state.json");
        symlink(&missing_target, &state).unwrap();
        let id = WorkspaceId::new("must-not-reserve").unwrap();

        assert!(repository.list().is_err());
        assert!(repository.get(&id).is_err());
        assert!(repository.reserve(&id, "new-request").is_err());
        assert_eq!(fs::read_link(&state).unwrap(), missing_target);
        assert!(!missing_target.exists());
    }
    #[cfg(unix)]
    #[test]
    fn live_state_symlink_is_rejected_without_touching_its_target() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("state");
        let repository = FileStateRepository::new(&root).unwrap();
        let target = temporary.path().join("external-state.json");
        let bytes = serde_json::to_vec(&RepositoryState::default()).unwrap();
        fs::write(&target, &bytes).unwrap();
        let state = root.join("state.json");
        symlink(&target, &state).unwrap();

        assert!(repository.list().is_err());
        assert!(repository
            .reserve(
                &WorkspaceId::new("must-not-reserve").unwrap(),
                "new-request"
            )
            .is_err());
        assert_eq!(fs::read_link(state).unwrap(), target);
        assert_eq!(fs::read(target).unwrap(), bytes);
    }
    #[test]
    fn state_directory_is_rejected_without_replacing_its_contents() {
        let temporary = tempfile::tempdir().unwrap();
        let repository = FileStateRepository::new(temporary.path()).unwrap();
        let state = temporary.path().join("state.json");
        fs::create_dir(&state).unwrap();
        fs::write(state.join("preserved"), b"keep this state").unwrap();

        assert!(repository.list().is_err());
        assert!(repository
            .reserve(
                &WorkspaceId::new("must-not-reserve").unwrap(),
                "new-request"
            )
            .is_err());
        assert_eq!(
            fs::read(state.join("preserved")).unwrap(),
            b"keep this state"
        );
    }
    #[test]
    fn corrupt_or_unsupported_state_is_never_reset_by_read_or_write() {
        let temporary = tempfile::tempdir().unwrap();
        let repository = FileStateRepository::new(temporary.path()).unwrap();
        let state = temporary.path().join("state.json");
        let id = WorkspaceId::new("must-not-reserve").unwrap();
        let unsupported = RepositoryState {
            version: 999,
            ..RepositoryState::default()
        };
        for bytes in [
            b"{ corrupt state".to_vec(),
            b"{}".to_vec(),
            serde_json::to_vec(&unsupported).unwrap(),
        ] {
            fs::write(&state, &bytes).unwrap();
            assert!(repository.list().is_err());
            assert!(repository.get(&id).is_err());
            assert!(repository.reserve(&id, "new-request").is_err());
            assert_eq!(fs::read(&state).unwrap(), bytes);
        }
    }
    #[test]
    fn oversized_state_is_rejected_without_truncating_it() {
        let temporary = tempfile::tempdir().unwrap();
        let repository = FileStateRepository::new(temporary.path()).unwrap();
        let state = temporary.path().join("state.json");
        let file = File::create(&state).unwrap();
        let length = 32 * 1024 * 1024 + 1;
        file.set_len(length).unwrap();
        drop(file);

        assert!(matches!(repository.list(), Err(Error::State(_))));
        assert!(matches!(
            repository.reserve(
                &WorkspaceId::new("must-not-reserve").unwrap(),
                "new-request"
            ),
            Err(Error::State(_))
        ));
        assert_eq!(fs::metadata(state).unwrap().len(), length);
    }
    #[test]
    fn absent_state_reads_are_empty_and_do_not_create_a_state_file() {
        let temporary = tempfile::tempdir().unwrap();
        let repository = FileStateRepository::new(temporary.path()).unwrap();
        let id = WorkspaceId::new("fresh").unwrap();

        assert!(repository.list().unwrap().is_empty());
        assert!(repository.get(&id).unwrap().is_none());
        assert!(!temporary.path().join("state.json").exists());
        repository.reserve(&id, "request").unwrap();
        assert!(temporary.path().join("state.json").is_file());
    }
    #[test]
    fn legacy_state_upgrades_only_after_a_successful_write_and_keeps_reservations() {
        let temporary = tempfile::tempdir().unwrap();
        let repository = FileStateRepository::new(temporary.path()).unwrap();
        let id = WorkspaceId::new("reserved-before-upgrade").unwrap();
        let legacy = serde_json::json!({
            "version": 1,
            "reservations": {"reserved-before-upgrade": "original-request"},
            "workspaces": {},
            "workflows": {},
        });
        let bytes = serde_json::to_vec(&legacy).unwrap();
        let path = temporary.path().join("state.json");
        fs::write(&path, &bytes).unwrap();

        assert!(repository.list().unwrap().is_empty());
        // An unfinished legacy reservation is a create that may have reached the
        // provider: it reads as Provisioning under the key Boat was given.
        let record = repository.record(&id).unwrap().unwrap();
        assert_eq!(record.origin, RequestOrigin::Upgraded);
        assert_eq!(
            record.state,
            MachineState::Provisioning {
                key: ProvisionKey::for_workspace(&id)
            }
        );
        // Reading never rewrites the file.
        assert_eq!(fs::read(&path).unwrap(), bytes);

        // The first request adopts the upgraded record and saves it as v4.
        let adopted = repository
            .begin(
                &id,
                "first-request",
                MachineState::Provisioning {
                    key: ProvisionKey::for_workspace(&id),
                },
            )
            .unwrap();
        assert_eq!(
            adopted.origin,
            RequestOrigin::Request {
                hash: "first-request".into()
            }
        );
        let upgraded: RepositoryState = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(upgraded.version, VERSION);
        assert_eq!(upgraded.machines.get(&id), Some(&adopted));
        // Once adopted, it is that request's record.
        assert!(matches!(
            repository.reserve(&id, "different-request"),
            Err(Error::Conflict(_))
        ));
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
