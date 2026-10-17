//! Native restart and independent-store simulation using existing public ports.
//! Separate temporary roots model placement; they do not prove physical off-host
//! durability, power-loss persistence, or distributed authority semantics.
mod support;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tempfile::TempDir;
use workspace_sdk::{
    checkpoint::{
        CaptureOptions, CheckpointBundle, CheckpointRef, GitCheckpointEngine, ICheckpointEngine,
    },
    infrastructure::FileStateRepository,
    ports::{IRecoveryRepository, IWorkflowLease},
    providers::{LocalConfig, LocalProvider},
    CommandSpec, Error, OperationId, ProviderId, ProviderOptions, RecoveryCandidate, RecoveryPhase,
    RecoveryRequest, RecoverySafety, RecoveryService, RecoverySession, ResourceRequirements,
    Result, RunRef, RunState, SyncService, WorkspaceSdk,
};

fn provider() -> ProviderId {
    ProviderId::new("replacement").unwrap()
}
fn request() -> RecoveryRequest {
    RecoveryRequest {
        request_id: OperationId::new("continue-on-replacement").unwrap(),
        expected_previous_epoch: 0,
        available: vec![provider()],
        continuation: CommandSpec::new("./worker")
            .args(["increment", "counter", "1"])
            .into(),
        safety: RecoverySafety::ConfirmedStopped,
    }
}
fn controller(root: &Path) -> (Arc<WorkspaceSdk>, Arc<FileStateRepository>) {
    let registry = Arc::new(FileStateRepository::new(root.join("registry")).unwrap());
    let mut config = LocalConfig::trusted_host(root.join("workspaces"));
    config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
    let sdk = WorkspaceSdk::builder(registry.clone())
        .provider(Arc::new(
            LocalProvider::new(config).unwrap().with_id(provider()),
        ))
        .unwrap()
        .build()
        .unwrap();
    (Arc::new(sdk), registry)
}

struct IndependentStores {
    laptop: TempDir,
    replica: TempDir,
    authority: TempDir,
    replacement: TempDir,
    id: OperationId,
    checkpoint: CheckpointRef,
}
impl IndependentStores {
    async fn seed() -> Self {
        let laptop = TempDir::new().unwrap();
        let replica = TempDir::new().unwrap();
        let authority = TempDir::new().unwrap();
        let replacement = TempDir::new().unwrap();
        let project = laptop.path().join("project");
        fs::create_dir(&project).unwrap();
        fs::write(project.join("counter"), "0").unwrap();
        fs::write(project.join("payload.bin"), [0, 255, 1, 128]).unwrap();
        // The continuation executable travels inside the selected checkpoint;
        // recovery cannot depend on the source project's executable path.
        fs::copy(support::workload_program(), project.join("worker")).unwrap();
        let local = GitCheckpointEngine::new("git", laptop.path().join("checkpoints")).unwrap();
        let captured = local.capture(&project, CaptureOptions::default()).unwrap();
        let exported = local.export(captured.reference()).unwrap();
        // Serialize the complete public transport envelope, including its manifest.
        let transferred: CheckpointBundle =
            serde_json::from_slice(&serde_json::to_vec(&exported).unwrap()).unwrap();
        let remote =
            Arc::new(GitCheckpointEngine::new("git", replica.path().join("checkpoints")).unwrap());
        let imported = remote.import(&transferred).unwrap();
        assert_eq!(imported.reference(), captured.reference());
        assert_eq!(
            imported.manifest.tree_fingerprint,
            captured.manifest.tree_fingerprint
        );
        let (sdk, registry) = controller(laptop.path());
        let authority_repository = Arc::new(FileStateRepository::new(authority.path()).unwrap());
        let recovery = RecoveryService::new(
            sdk,
            Arc::new(SyncService::new(remote, registry)),
            authority_repository,
        );
        let id = OperationId::new("independent-store-session").unwrap();
        let saved = recovery
            .create(
                id.clone(),
                vec![RecoveryCandidate {
                    provider: provider(),
                    config: ProviderOptions::default(),
                    resources: ResourceRequirements::default(),
                }],
                imported.reference().clone(),
            )
            .await
            .unwrap();
        assert_eq!(saved.committed.checkpoint, *imported.reference());
        assert!(saved.current_attempt.is_none());
        Self {
            laptop,
            replica,
            authority,
            replacement,
            id,
            checkpoint: imported.reference().clone(),
        }
    }
    fn engine(&self) -> Arc<GitCheckpointEngine> {
        Arc::new(GitCheckpointEngine::new("git", self.replica.path().join("checkpoints")).unwrap())
    }
    fn authority(&self) -> Arc<FileStateRepository> {
        Arc::new(FileStateRepository::new(self.authority.path()).unwrap())
    }
    fn reopen(&self) -> (Arc<WorkspaceSdk>, RecoveryService) {
        let (sdk, registry) = controller(self.replacement.path());
        let recovery = RecoveryService::new(
            sdk.clone(),
            Arc::new(SyncService::new(self.engine(), registry)),
            self.authority(),
        );
        (sdk, recovery)
    }
    fn assert_laptop_discarded(&self) {
        for name in ["project", "checkpoints", "registry"] {
            assert!(
                !self.laptop.path().join(name).exists(),
                "recovery must not recreate source state"
            );
        }
    }
    fn discard_laptop_state(&self) {
        // These are exclusively owned disposable source locations, not survivor
        // stores or native target state. Keep all removal paths explicit.
        for name in ["project", "checkpoints", "registry"] {
            let path = self.laptop.path().join(name);
            assert!(path.is_dir());
            fs::remove_dir_all(&path).unwrap();
            assert!(!path.exists());
        }
        assert!(self.replica.path().join("checkpoints").is_dir());
        assert!(self.authority.path().join("state.json").is_file());
    }
}
fn started(session: &RecoverySession) -> RunRef {
    match &session.current_attempt.as_ref().unwrap().phase {
        RecoveryPhase::Started { run, .. } => run.clone(),
        phase => panic!("expected durable started authority, got {phase:?}"),
    }
}
fn assert_same_run(actual: &RunRef, expected: &RunRef) {
    assert_eq!(actual.workspace, expected.workspace);
    assert_eq!(actual.id, expected.id);
}
async fn finished(sdk: &WorkspaceSdk, session: &RecoverySession) -> PathBuf {
    let run = started(session);
    let workspace = sdk.connect(&run.workspace).await.unwrap();
    assert_eq!(
        workspace
            .run(run.id)
            .wait(Duration::from_secs(10))
            .await
            .unwrap(),
        RunState::Exited { exit_code: 0 }
    );
    let root = PathBuf::from(workspace.descriptor().root);
    assert_eq!(fs::read_to_string(root.join("counter")).unwrap(), "1");
    assert_eq!(
        fs::read(root.join("payload.bin")).unwrap(),
        [0, 255, 1, 128]
    );
    root
}

#[tokio::test]
async fn imported_checkpoint_and_independent_authority_survive_discarded_laptop_state() {
    let stores = IndependentStores::seed().await;
    stores.discard_laptop_state();
    let (sdk, recovery) = stores.reopen();
    assert!(
        sdk.list().unwrap().is_empty(),
        "new controller has no source workspace registry"
    );
    let saved = recovery.session(&stores.id).unwrap();
    assert_eq!(saved.committed.checkpoint, stores.checkpoint);
    let accepted = recovery.recover(&stores.id, request()).await.unwrap();
    let run = started(&accepted);
    let root = finished(&sdk, &accepted).await;
    assert!(root.starts_with(stores.replacement.path().canonicalize().unwrap()));
    // A post-launch marker must survive reconnect: retry may not restore the seed.
    fs::write(root.join("after-launch"), "retain").unwrap();
    drop(recovery);
    drop(sdk);
    let (sdk, recovery) = stores.reopen();
    let retried = recovery.recover(&stores.id, request()).await.unwrap();
    assert_same_run(&started(&retried), &run);
    assert_eq!(finished(&sdk, &retried).await, root);
    assert_eq!(
        fs::read_to_string(root.join("after-launch")).unwrap(),
        "retain"
    );
    assert_eq!(
        sdk.list().unwrap().len(),
        1,
        "retry must retain one workspace and one native launch"
    );
    stores.assert_laptop_discarded();
}

#[tokio::test]
async fn rejected_replica_imports_leave_previous_committed_checkpoint_recoverable() {
    let stores = IndependentStores::seed().await;
    let before = fs::read(stores.authority.path().join("state.json")).unwrap();
    let project = stores.laptop.path().join("project");
    fs::write(project.join("counter"), "20").unwrap();
    let source = GitCheckpointEngine::new("git", stores.laptop.path().join("checkpoints")).unwrap();
    let newer = source.capture(&project, CaptureOptions::default()).unwrap();
    let bundle = source.export(newer.reference()).unwrap();
    let remote = stores.engine();
    let mut corrupt = bundle.clone();
    let last = corrupt.bytes.len() - 1;
    corrupt.bytes[last] ^= 1;
    assert!(remote.import(&corrupt).is_err());
    let mut truncated = bundle.clone();
    truncated.bytes.truncate(truncated.bytes.len() / 2);
    assert!(remote.import(&truncated).is_err());
    assert!(
        remote.export(newer.reference()).is_err(),
        "failed import must not publish an incomplete checkpoint"
    );
    assert!(remote.export(&stores.checkpoint).is_ok());
    assert_eq!(
        fs::read(stores.authority.path().join("state.json")).unwrap(),
        before,
        "artifact rejection must not advance committed authority"
    );
    drop(source);
    drop(remote);
    stores.discard_laptop_state();
    let (sdk, recovery) = stores.reopen();
    assert_eq!(
        recovery.session(&stores.id).unwrap().committed.checkpoint,
        stores.checkpoint
    );
    let accepted = recovery.recover(&stores.id, request()).await.unwrap();
    finished(&sdk, &accepted).await;
    stores.assert_laptop_discarded();
}

/// Model an authority CAS that commits successfully but loses its caller acknowledgment.
struct LostStartedAcknowledgment {
    inner: Arc<FileStateRepository>,
    armed: AtomicBool,
}
impl IRecoveryRepository for LostStartedAcknowledgment {
    fn acquire_recovery(&self, id: &OperationId) -> Result<Box<dyn IWorkflowLease>> {
        self.inner.acquire_recovery(id)
    }
    fn load_recovery(&self, id: &OperationId) -> Result<Option<RecoverySession>> {
        self.inner.load_recovery(id)
    }
    fn compare_exchange_recovery(
        &self,
        id: &OperationId,
        previous: Option<u64>,
        next: &RecoverySession,
    ) -> Result<()> {
        self.inner.compare_exchange_recovery(id, previous, next)?;
        if next
            .current_attempt
            .as_ref()
            .is_some_and(|attempt| matches!(attempt.phase, RecoveryPhase::Started { .. }))
            && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(Error::State(
                "fixture lost committed authority acknowledgment".into(),
            ));
        }
        Ok(())
    }
}

#[tokio::test]
async fn independent_authority_inspection_resolves_lost_ack_without_repeating_increment() {
    let stores = IndependentStores::seed().await;
    stores.discard_laptop_state();
    let (sdk, registry) = controller(stores.replacement.path());
    let fault = Arc::new(LostStartedAcknowledgment {
        inner: stores.authority(),
        armed: AtomicBool::new(true),
    });
    let recovery = RecoveryService::new(
        sdk.clone(),
        Arc::new(SyncService::new(stores.engine(), registry)),
        fault.clone(),
    );
    let error = recovery.recover(&stores.id, request()).await.unwrap_err();
    assert!(
        matches!(error, Error::State(message) if message == "fixture lost committed authority acknowledgment")
    );
    assert!(
        !fault.armed.load(Ordering::SeqCst),
        "the acknowledgment fault must be exercised"
    );
    let observed = stores
        .authority()
        .load_recovery(&stores.id)
        .unwrap()
        .unwrap();
    let run = started(&observed);
    let root = finished(&sdk, &observed).await;
    fs::write(root.join("after-launch"), "retain").unwrap();
    drop(recovery);
    drop(fault);
    drop(sdk);
    let (sdk, recovery) = stores.reopen();
    assert_same_run(&started(&recovery.session(&stores.id).unwrap()), &run);
    let retried = recovery.recover(&stores.id, request()).await.unwrap();
    assert_same_run(&started(&retried), &run);
    assert_eq!(finished(&sdk, &retried).await, root);
    assert_eq!(
        fs::read_to_string(root.join("after-launch")).unwrap(),
        "retain"
    );
    assert_eq!(sdk.list().unwrap().len(), 1);
    stores.assert_laptop_discarded();
}
