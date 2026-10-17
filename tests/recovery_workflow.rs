//! Real local native processes and checkpoint transfers; availability and provider
//! names simulate placement only. This does not qualify physical host loss or Boat.
mod support;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tempfile::TempDir;
use workspace_sdk::{
    checkpoint::{CaptureOptions, CheckpointRef, GitCheckpointEngine, ICheckpointEngine},
    infrastructure::FileStateRepository,
    ports::{IRecoveryRepository, IWorkflowLease},
    providers::{LocalConfig, LocalOpenOptions, LocalProvider},
    *,
};

const LOCATIONS: [&str; 3] = ["laptop", "devbox", "boat-sim"];

fn provider(name: &str) -> ProviderId {
    ProviderId::new(name).unwrap()
}
fn operation(name: &str) -> OperationId {
    OperationId::new(name).unwrap()
}
fn candidates() -> Vec<RecoveryCandidate> {
    LOCATIONS
        .iter()
        .map(|name| RecoveryCandidate {
            provider: provider(name),
            config: ProviderOptions::default(),
            resources: ResourceRequirements::default(),
        })
        .collect()
}
fn controller(root: &Path, repository: Arc<FileStateRepository>) -> Arc<WorkspaceSdk> {
    let mut builder = WorkspaceSdk::builder(repository);
    for location in LOCATIONS {
        let mut config = LocalConfig::trusted_host(root.join(location));
        config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
        builder = builder
            .provider(Arc::new(
                LocalProvider::new(config)
                    .unwrap()
                    .with_id(provider(location)),
            ))
            .unwrap();
    }
    Arc::new(builder.build().unwrap())
}
fn continuation() -> ExecutionRequest {
    CommandSpec::new(support::workload_program())
        .args(["increment", "counter", "2"])
        .into()
}
fn request(id: &str, previous: u64, available: &[&str]) -> RecoveryRequest {
    RecoveryRequest {
        request_id: operation(id),
        expected_previous_epoch: previous,
        available: available.iter().map(|name| provider(name)).collect(),
        continuation: continuation(),
        safety: RecoverySafety::ConfirmedStopped,
    }
}
fn assert_same_run(left: &RecoverySession, right: &RecoverySession) {
    let left = started(left);
    let right = started(right);
    assert_eq!(left.workspace, right.workspace);
    assert_eq!(left.id, right.id);
}
fn started(session: &RecoverySession) -> RunRef {
    match &session.current_attempt.as_ref().unwrap().phase {
        RecoveryPhase::Started { run, .. } => run.clone(),
        phase => panic!("expected accepted native run, got {phase:?}"),
    }
}
async fn finished(sdk: &WorkspaceSdk, session: &RecoverySession) -> PathBuf {
    let reference = started(session);
    let workspace = sdk.connect(&reference.workspace).await.unwrap();
    assert_eq!(
        workspace
            .run(reference.id)
            .wait(Duration::from_secs(10))
            .await
            .unwrap(),
        RunState::Exited { exit_code: 0 }
    );
    PathBuf::from(workspace.descriptor().root)
}
fn checkpoint(root: &Path, engine: &GitCheckpointEngine) -> CheckpointRef {
    let source = root.join("seed-project");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("counter"), "3").unwrap();
    engine
        .capture(&source, CaptureOptions::default())
        .unwrap()
        .reference()
        .clone()
}
fn service(
    sdk: Arc<WorkspaceSdk>,
    engine: Arc<GitCheckpointEngine>,
    repository: Arc<FileStateRepository>,
) -> RecoveryService {
    RecoveryService::new(
        sdk,
        Arc::new(SyncService::new(engine, repository.clone())),
        repository,
    )
}

#[tokio::test]
async fn priority_recovery_moves_complete_checkpoints_and_preserves_old_workspaces() {
    let temp = TempDir::new().unwrap();
    let state = temp.path().join("state");
    let repository = Arc::new(FileStateRepository::new(&state).unwrap());
    let sdk = controller(temp.path(), repository.clone());
    let engine = Arc::new(GitCheckpointEngine::new("git", temp.path().join("artifacts")).unwrap());
    let seed = checkpoint(temp.path(), &engine);
    let recovery = service(sdk.clone(), engine.clone(), repository);
    let id = operation("priority-flow");
    recovery
        .create(id.clone(), candidates(), seed)
        .await
        .unwrap();
    let mut roots = Vec::new();
    let mut prior_head = recovery.session(&id).unwrap().committed;
    for (index, (expected, available)) in [
        ("laptop", vec!["boat-sim", "devbox", "laptop"]),
        ("devbox", vec!["boat-sim", "devbox"]),
        ("boat-sim", vec!["boat-sim"]),
        ("laptop", vec!["laptop", "devbox", "boat-sim"]),
    ]
    .iter()
    .enumerate()
    {
        let previous = index as u64;
        let request_id = format!("switch-{index}");
        let accepted = recovery
            .recover(&id, request(&request_id, previous, available))
            .await
            .unwrap();
        let attempt = accepted.current_attempt.as_ref().unwrap();
        assert_eq!(attempt.epoch, previous + 1);
        assert_eq!(attempt.provider, provider(expected));
        assert_eq!(attempt.checkpoint, prior_head);
        let root = finished(&sdk, &accepted).await;
        assert_eq!(
            fs::read_to_string(root.join("counter")).unwrap(),
            (5 + index * 2).to_string()
        );
        assert!(!roots.contains(&root));
        roots.push(root);
        // A priority change on an exact retry reconnects the pinned target; it
        // cannot move a completed launch or repeat its side effect.
        let retry = recovery
            .recover(&id, request(&request_id, previous, &LOCATIONS))
            .await
            .unwrap();
        assert_same_run(&retry, &accepted);
        assert!(recovery.publish(&id, previous + 1, false).await.is_err());
        let published = recovery.publish(&id, previous + 1, true).await.unwrap();
        assert_eq!(published.committed.generation, previous + 1);
        assert_ne!(published.committed.checkpoint, prior_head.checkpoint);
        prior_head = published.committed;
        if index > 0 {
            assert!(recovery.publish(&id, previous, true).await.is_err());
            assert!(recovery
                .recover(&id, request("switch-0", 0, &LOCATIONS))
                .await
                .is_err());
        }
    }
    for (index, root) in roots.iter().enumerate() {
        assert_eq!(
            fs::read_to_string(root.join("counter")).unwrap(),
            (5 + index * 2).to_string()
        );
    }
    assert_eq!(
        fs::read_to_string(temp.path().join("seed-project/counter")).unwrap(),
        "3"
    );
    drop(recovery);
    drop(sdk);
    let reopened = Arc::new(FileStateRepository::new(&state).unwrap());
    let sdk = controller(temp.path(), reopened.clone());
    let restored = service(sdk, engine, reopened).session(&id).unwrap();
    assert_eq!(restored.committed, prior_head);
    assert_eq!(restored.current_attempt.unwrap().epoch, 4);
}

#[tokio::test]
async fn confirmed_stopped_refuses_active_run_and_leaves_authority_unchanged() {
    let temp = TempDir::new().unwrap();
    let repository = Arc::new(FileStateRepository::new(temp.path().join("state")).unwrap());
    let sdk = controller(temp.path(), repository.clone());
    let engine = Arc::new(GitCheckpointEngine::new("git", temp.path().join("artifacts")).unwrap());
    let seed = checkpoint(temp.path(), &engine);
    let recovery = service(sdk.clone(), engine, repository);
    let id = operation("active-refusal");
    recovery
        .create(id.clone(), candidates(), seed)
        .await
        .unwrap();
    let mut initial = request("start", 0, &["laptop"]);
    initial.continuation = CommandSpec::new(support::workload_program())
        .args(["sleep", "30"])
        .into();
    let accepted = recovery.recover(&id, initial).await.unwrap();
    let reference = started(&accepted);
    let workspace = sdk.connect(&reference.workspace).await.unwrap();
    let run = workspace.run(reference.id);
    let refused = recovery
        .recover(&id, request("switch", 1, &["devbox"]))
        .await;
    // Clean up before assertions so a failed refusal test does not leave its
    // native process alive for the full workload duration.
    run.cancel().await.unwrap();
    run.wait(Duration::from_secs(10)).await.unwrap();
    assert!(refused.is_err());
    assert!(recovery.publish(&id, 0, true).await.is_err());
    let current = recovery.session(&id).unwrap();
    assert_eq!(current.revision, accepted.revision);
    assert_same_run(&current, &accepted);
}

#[tokio::test]
async fn missing_artifact_and_empty_availability_do_not_create_targets() {
    let temp = TempDir::new().unwrap();
    let repository = Arc::new(FileStateRepository::new(temp.path().join("state")).unwrap());
    let sdk = controller(temp.path(), repository.clone());
    let engine = Arc::new(GitCheckpointEngine::new("git", temp.path().join("artifacts")).unwrap());
    let recovery = service(sdk.clone(), engine.clone(), repository);
    let missing = operation("missing-artifact");
    assert!(recovery
        .create(
            missing.clone(),
            candidates(),
            CheckpointRef("missing".into())
        )
        .await
        .is_err());
    assert!(matches!(
        recovery.session(&missing),
        Err(Error::NotFound(_))
    ));
    let id = operation("no-placement");
    let initial = recovery
        .create(id.clone(), candidates(), checkpoint(temp.path(), &engine))
        .await
        .unwrap();
    assert!(matches!(
        recovery.recover(&id, request("switch", 0, &[])).await,
        Err(Error::NotFound(_))
    ));
    assert_eq!(recovery.session(&id).unwrap().revision, initial.revision);
    assert!(recovery.session(&id).unwrap().current_attempt.is_none());
    assert!(sdk.list().unwrap().is_empty());
}

#[tokio::test]
async fn externally_fenced_replacement_keeps_old_process_and_rejects_its_publication() {
    let temp = TempDir::new().unwrap();
    let repository = Arc::new(FileStateRepository::new(temp.path().join("state")).unwrap());
    let sdk = controller(temp.path(), repository.clone());
    let engine = Arc::new(GitCheckpointEngine::new("git", temp.path().join("artifacts")).unwrap());
    let seed = checkpoint(temp.path(), &engine);
    let recovery = service(sdk.clone(), engine, repository);
    let id = operation("external-fence");
    recovery
        .create(id.clone(), candidates(), seed)
        .await
        .unwrap();
    let mut initial = request("start", 0, &["laptop"]);
    initial.continuation = CommandSpec::new(support::workload_program())
        .args(["sleep", "30"])
        .into();
    let accepted = recovery.recover(&id, initial).await.unwrap();
    let reference = started(&accepted);
    let old_workspace = sdk.connect(&reference.workspace).await.unwrap();
    let old_run = old_workspace.run(reference.id);
    let mut replacement = request("switch", 1, &["devbox"]);
    // This is a caller assertion exercised by the test, not evidence that the
    // SDK fenced an arbitrary external API or stopped the old native process.
    replacement.safety = RecoverySafety::ExternallyFenced;
    let result = recovery.recover(&id, replacement).await;
    let still_active = old_run.state().await.unwrap();
    old_run.cancel().await.unwrap();
    old_run.wait(Duration::from_secs(10)).await.unwrap();
    let replaced = result.unwrap();
    assert!(matches!(
        still_active,
        RunState::Running | RunState::Starting
    ));
    assert_eq!(replaced.current_attempt.as_ref().unwrap().epoch, 2);
    assert_eq!(
        replaced.current_attempt.as_ref().unwrap().provider,
        provider("devbox")
    );
    assert!(recovery.publish(&id, 1, true).await.is_err());
    let root = finished(&sdk, &replaced).await;
    assert_eq!(fs::read_to_string(root.join("counter")).unwrap(), "5");
    assert_eq!(
        fs::read_to_string(Path::new(&old_workspace.descriptor().root).join("counter")).unwrap(),
        "3"
    );
}

#[tokio::test]
async fn attached_project_is_refused_before_checkpoint_restore_or_native_launch() {
    let temp = TempDir::new().unwrap();
    let repository = Arc::new(FileStateRepository::new(temp.path().join("state")).unwrap());
    let sdk = controller(temp.path(), repository.clone());
    let engine = Arc::new(GitCheckpointEngine::new("git", temp.path().join("artifacts")).unwrap());
    let seed = checkpoint(temp.path(), &engine);
    let source = temp.path().join("seed-project");
    let mut locations = candidates();
    locations[0].config = ProviderOptions::from_options(&LocalOpenOptions {
        attach: Some(source.clone()),
    })
    .unwrap();
    let recovery = service(sdk.clone(), engine, repository);
    let id = operation("attached-project");
    recovery.create(id.clone(), locations, seed).await.unwrap();
    assert!(matches!(
        recovery.recover(&id, request("switch", 0, &["laptop"])).await,
        Err(Error::Conflict(message)) if message.contains("SDK-owned workspace")
    ));
    assert_eq!(fs::read_to_string(source.join("counter")).unwrap(), "3");
    let snapshot = recovery.session(&id).unwrap();
    let attempt = snapshot.current_attempt.as_ref().unwrap();
    assert!(matches!(attempt.phase, RecoveryPhase::Preparing));
    let target = sdk.workspace(&attempt.workspace_id).await.unwrap();
    assert_eq!(target.descriptor().ownership, Ownership::Attached);
    assert_eq!(
        PathBuf::from(target.descriptor().root),
        source.canonicalize().unwrap()
    );
    assert_no_native_claim(&target);
    assert!(matches!(
        recovery.recover(&id, request("next", 1, &["devbox"])).await,
        Err(Error::Conflict(_))
    ));
    let after = recovery.session(&id).unwrap();
    assert_eq!(after.revision, snapshot.revision);
    assert_eq!(after.current_attempt.unwrap().epoch, 1);
    assert_eq!(fs::read_to_string(source.join("counter")).unwrap(), "3");
}

fn with_transient_inputs(mut request: RecoveryRequest, suffix: &str) -> RecoveryRequest {
    request.continuation.secrets.0.insert(
        "RECOVERY_FIXTURE_TOKEN".into(),
        format!("fixture-private-token-{suffix}"),
    );
    request.continuation.private_stdin = Some(SecretInput::new(
        format!("fixture-private-input-{suffix}").into_bytes(),
    ));
    request
}
fn assert_no_transient_plaintext(state: &Path) {
    let persisted = fs::read_to_string(state.join("state.json")).unwrap();
    assert!(!persisted.contains("fixture-private-token"));
    assert!(!persisted.contains("fixture-private-input"));
}
fn launch_identity(session: &RecoverySession) -> &str {
    match &session.current_attempt.as_ref().unwrap().phase {
        RecoveryPhase::Started {
            launch_identity, ..
        } => launch_identity,
        _ => panic!("expected started receipt"),
    }
}
fn assert_no_native_claim(workspace: &Workspace) {
    let runs = Path::new(&workspace.descriptor().control_root).join("runs");
    if runs.exists() {
        assert!(fs::read_dir(runs).unwrap().all(|entry| !entry
            .unwrap()
            .path()
            .join("claim.json")
            .exists()));
    }
}

#[tokio::test]
async fn started_retry_does_not_require_old_transient_credentials_or_hash_them_in_authority() {
    let temp = TempDir::new().unwrap();
    let state = temp.path().join("state");
    let repository = Arc::new(FileStateRepository::new(&state).unwrap());
    let sdk = controller(temp.path(), repository.clone());
    let engine = Arc::new(GitCheckpointEngine::new("git", temp.path().join("artifacts")).unwrap());
    let seed = checkpoint(temp.path(), &engine);
    let recovery = service(sdk.clone(), engine.clone(), repository);
    let id = operation("transient-inputs");
    recovery
        .create(id.clone(), candidates(), seed.clone())
        .await
        .unwrap();
    let accepted = recovery
        .recover(
            &id,
            with_transient_inputs(request("switch", 0, &["laptop"]), "first"),
        )
        .await
        .unwrap();
    let root = finished(&sdk, &accepted).await;
    assert_no_transient_plaintext(&state);
    drop(recovery);
    drop(sdk);
    let reopened = Arc::new(FileStateRepository::new(&state).unwrap());
    let sdk = controller(temp.path(), reopened.clone());
    let recovery = service(sdk.clone(), engine, reopened);
    let changed = recovery
        .recover(
            &id,
            with_transient_inputs(request("switch", 0, &["devbox"]), "rotated"),
        )
        .await
        .unwrap();
    let omitted = recovery
        .recover(&id, request("switch", 0, &LOCATIONS))
        .await
        .unwrap();
    assert_same_run(&accepted, &changed);
    assert_same_run(&accepted, &omitted);
    // Compare behaviorally equivalent commands in distinct sessions rather than
    // duplicating the implementation's hash algorithm as a test oracle.
    let plain_id = operation("command-only");
    recovery
        .create(plain_id.clone(), candidates(), seed)
        .await
        .unwrap();
    let plain = recovery
        .recover(&plain_id, request("switch", 0, &["laptop"]))
        .await
        .unwrap();
    finished(&sdk, &plain).await;
    assert_eq!(launch_identity(&accepted), launch_identity(&plain));
    assert_no_transient_plaintext(&state);
    assert_eq!(fs::read_to_string(root.join("counter")).unwrap(), "5");
}

#[tokio::test]
async fn ambiguous_native_launch_still_requires_original_transient_inputs_until_started_is_saved() {
    let temp = TempDir::new().unwrap();
    let state = temp.path().join("state");
    let repository = Arc::new(FileStateRepository::new(&state).unwrap());
    let sdk = controller(temp.path(), repository.clone());
    let engine = Arc::new(GitCheckpointEngine::new("git", temp.path().join("artifacts")).unwrap());
    let seed = checkpoint(temp.path(), &engine);
    let id = operation("transient-uncertain-launch");
    let fault = Arc::new(ReceiptFault {
        inner: repository.clone(),
        id: id.clone(),
        phase: FaultPhase::Started,
        commit_before_error: false,
        fail_next: AtomicBool::new(true),
        attempted: Mutex::new(None),
    });
    let recovery = RecoveryService::new(
        sdk.clone(),
        Arc::new(SyncService::new(engine.clone(), repository.clone())),
        fault.clone(),
    );
    recovery
        .create(id.clone(), candidates(), seed)
        .await
        .unwrap();
    assert!(matches!(
        recovery.recover(&id, with_transient_inputs(request("switch", 0, &["laptop"]), "first")).await,
        Err(Error::State(message)) if message == "injected recovery receipt failure"
    ));
    let attempted = fault.attempted.lock().unwrap().clone().unwrap();
    let root = finished(&sdk, &attempted).await;
    assert_no_transient_plaintext(&state);
    drop(recovery);
    drop(fault);
    drop(repository);
    drop(sdk);
    let reopened = Arc::new(FileStateRepository::new(&state).unwrap());
    let sdk = controller(temp.path(), reopened.clone());
    let recovery = service(sdk.clone(), engine, reopened);
    let saved = recovery.session(&id).unwrap();
    assert!(matches!(
        saved.current_attempt.as_ref().unwrap().phase,
        RecoveryPhase::LaunchRequested { .. }
    ));
    let changed = recovery
        .recover(
            &id,
            with_transient_inputs(request("switch", 0, &LOCATIONS), "rotated"),
        )
        .await;
    // This message comes from the native salted launch claim. A service-level
    // command fingerprint mismatch would not establish native secret binding.
    assert!(changed
        .unwrap_err()
        .to_string()
        .contains("run ID was already claimed for a different execution"));
    assert_eq!(recovery.session(&id).unwrap().revision, saved.revision);
    assert_eq!(fs::read_to_string(root.join("counter")).unwrap(), "5");
    let recovered = recovery
        .recover(
            &id,
            with_transient_inputs(request("switch", 0, &LOCATIONS), "first"),
        )
        .await
        .unwrap();
    assert_same_run(&attempted, &recovered);
    let omitted = recovery
        .recover(&id, request("switch", 0, &LOCATIONS))
        .await
        .unwrap();
    assert_same_run(&recovered, &omitted);
    assert_eq!(fs::read_to_string(root.join("counter")).unwrap(), "5");
    assert_no_transient_plaintext(&state);
}

#[derive(Clone, Copy, Debug)]
enum FaultPhase {
    Restored,
    LaunchRequested,
    Started,
}
impl FaultPhase {
    fn matches(self, phase: &RecoveryPhase) -> bool {
        matches!(
            (self, phase),
            (Self::Restored, RecoveryPhase::Restored { .. })
                | (Self::LaunchRequested, RecoveryPhase::LaunchRequested { .. })
                | (Self::Started, RecoveryPhase::Started { .. })
        )
    }
}
struct ReceiptFault {
    inner: Arc<FileStateRepository>,
    id: OperationId,
    phase: FaultPhase,
    commit_before_error: bool,
    fail_next: AtomicBool,
    attempted: Mutex<Option<RecoverySession>>,
}
impl IRecoveryRepository for ReceiptFault {
    fn acquire_recovery(&self, id: &OperationId) -> Result<Box<dyn IWorkflowLease>> {
        self.inner.acquire_recovery(id)
    }
    fn load_recovery(&self, id: &OperationId) -> Result<Option<RecoverySession>> {
        self.inner.load_recovery(id)
    }
    fn compare_exchange_recovery(
        &self,
        id: &OperationId,
        expected_revision: Option<u64>,
        replacement: &RecoverySession,
    ) -> Result<()> {
        let matches = id == &self.id
            && replacement
                .current_attempt
                .as_ref()
                .is_some_and(|attempt| self.phase.matches(&attempt.phase));
        if matches && self.fail_next.swap(false, Ordering::SeqCst) {
            *self.attempted.lock().unwrap() = Some(replacement.clone());
            if self.commit_before_error {
                self.inner
                    .compare_exchange_recovery(id, expected_revision, replacement)?;
            }
            return Err(Error::State("injected recovery receipt failure".into()));
        }
        self.inner
            .compare_exchange_recovery(id, expected_revision, replacement)
    }
}

#[tokio::test]
async fn recovery_retries_restore_receipt_failure_before_native_launch() {
    receipt_failure(FaultPhase::Restored, false).await;
}
#[tokio::test]
async fn recovery_persists_launch_intent_before_native_side_effect() {
    receipt_failure(FaultPhase::LaunchRequested, false).await;
}
#[tokio::test]
async fn recovery_retries_missing_started_receipt_without_restoring_or_relaunching() {
    receipt_failure(FaultPhase::Started, false).await;
}
#[tokio::test]
async fn recovery_retries_lost_started_ack_without_restoring_or_relaunching() {
    receipt_failure(FaultPhase::Started, true).await;
}
async fn receipt_failure(phase: FaultPhase, commit_before_error: bool) {
    let temp = TempDir::new().unwrap();
    let state = temp.path().join("state");
    let repository = Arc::new(FileStateRepository::new(&state).unwrap());
    let sdk = controller(temp.path(), repository.clone());
    let engine = Arc::new(GitCheckpointEngine::new("git", temp.path().join("artifacts")).unwrap());
    let seed = checkpoint(temp.path(), &engine);
    let id = operation("receipt-boundary");
    let fault = Arc::new(ReceiptFault {
        inner: repository.clone(),
        id: id.clone(),
        phase,
        commit_before_error,
        fail_next: AtomicBool::new(true),
        attempted: Mutex::new(None),
    });
    let recovery = RecoveryService::new(
        sdk.clone(),
        Arc::new(SyncService::new(engine.clone(), repository.clone())),
        fault.clone(),
    );
    recovery
        .create(id.clone(), candidates(), seed)
        .await
        .unwrap();
    assert!(matches!(
        recovery.recover(&id, request("switch", 0, &["laptop"])).await,
        Err(Error::State(message)) if message == "injected recovery receipt failure"
    ));
    let attempted = fault.attempted.lock().unwrap().clone().unwrap();
    let attempt = attempted.current_attempt.as_ref().unwrap();
    let workspace = sdk.workspace(&attempt.workspace_id).await.unwrap();
    let root = PathBuf::from(workspace.descriptor().root);
    let launched = matches!(phase, FaultPhase::Started);
    if launched {
        finished(&sdk, &attempted).await;
    } else {
        let runs = Path::new(&workspace.descriptor().control_root).join("runs");
        if runs.exists() {
            assert!(fs::read_dir(runs).unwrap().all(|entry| !entry
                .unwrap()
                .path()
                .join("claim.json")
                .exists()));
        }
    }
    assert_eq!(
        fs::read_to_string(root.join("counter")).unwrap(),
        if launched { "5" } else { "3" }
    );
    if launched {
        // A file created after the launch must survive a controller retry; blindly
        // restoring the pinned checkpoint would erase it or fail import.
        fs::write(root.join("after-launch"), "keep").unwrap();
    }
    drop(workspace);
    drop(recovery);
    drop(fault);
    drop(repository);
    drop(sdk);

    let reopened = Arc::new(FileStateRepository::new(&state).unwrap());
    let saved = reopened.load_recovery(&id).unwrap().unwrap();
    let saved_phase = &saved.current_attempt.as_ref().unwrap().phase;
    match (phase, commit_before_error) {
        (FaultPhase::Restored, false) => assert!(matches!(saved_phase, RecoveryPhase::Preparing)),
        (FaultPhase::LaunchRequested, false) => {
            assert!(matches!(saved_phase, RecoveryPhase::Restored { .. }))
        }
        (FaultPhase::Started, false) => {
            assert!(matches!(saved_phase, RecoveryPhase::LaunchRequested { .. }))
        }
        (FaultPhase::Started, true) => {
            assert!(matches!(saved_phase, RecoveryPhase::Started { .. }))
        }
        _ => unreachable!(),
    }
    let sdk = controller(temp.path(), reopened.clone());
    let recovery = service(sdk.clone(), engine, reopened);
    if launched {
        let mut rebound = request("switch", 0, &LOCATIONS);
        rebound.continuation = CommandSpec::new(support::workload_program())
            .args(["writefile", "unexpected", "changed command ran"])
            .into();
        assert!(matches!(
            recovery.recover(&id, rebound).await,
            Err(Error::Conflict(_))
        ));
        assert!(!root.join("unexpected").exists());
    }
    let recovered = recovery
        .recover(&id, request("switch", 0, &["devbox"]))
        .await
        .unwrap();
    assert_eq!(recovered.current_attempt.as_ref().unwrap().epoch, 1);
    assert_eq!(
        recovered.current_attempt.as_ref().unwrap().provider,
        provider("laptop")
    );
    assert_eq!(
        recovered.current_attempt.as_ref().unwrap().workspace_id,
        attempt.workspace_id
    );
    assert_eq!(
        recovered.current_attempt.as_ref().unwrap().run_id,
        attempt.run_id
    );
    finished(&sdk, &recovered).await;
    let replay = recovery
        .recover(&id, request("switch", 0, &LOCATIONS))
        .await
        .unwrap();
    assert_same_run(&recovered, &replay);
    if launched {
        assert_same_run(&attempted, &recovered);
        assert_eq!(
            fs::read_to_string(root.join("after-launch")).unwrap(),
            "keep"
        );
    }
    assert_eq!(fs::read_to_string(root.join("counter")).unwrap(), "5");
    assert_eq!(
        fs::read_to_string(temp.path().join("seed-project/counter")).unwrap(),
        "3"
    );
}
