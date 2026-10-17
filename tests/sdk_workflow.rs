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
    checkpoint::{ApplyOptions, GitCheckpointEngine, ReconciliationStatus},
    infrastructure::FileStateRepository,
    ports::{IWorkflowLease, IWorkflowRepository},
    providers::{LocalConfig, LocalOpenOptions, LocalProvider},
    *,
};

fn sdk(root: &Path, repository: Arc<FileStateRepository>) -> WorkspaceSdk {
    WorkspaceSdk::builder(repository)
        .provider(Arc::new({
            let mut config = LocalConfig::trusted_host(root.join("targets"));
            config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
            LocalProvider::new(config).unwrap()
        }))
        .unwrap()
        .build()
        .unwrap()
}
fn request(id: &str) -> OpenRequest {
    OpenRequest {
        workspace_id: WorkspaceId::new(id).unwrap(),
        config: ProviderOptions::default(),
        resources: ResourceRequirements::default(),
    }
}
fn run_request(id: &str, args: &[&str]) -> LaunchRequest {
    LaunchRequest {
        id: RunId::new(id).unwrap(),
        execution: CommandSpec::new(support::workload_program())
            .args(args.iter().copied())
            .into(),
        output_limit_bytes: 1024 * 1024,
    }
}

#[tokio::test]
async fn persistent_reconnect_and_creation_identity_survive_controller_replacement() {
    let temp = TempDir::new().unwrap();
    let repo = Arc::new(FileStateRepository::new(temp.path().join("state")).unwrap());
    let first = sdk(temp.path(), repo.clone());
    let workspace = first
        .open(&ProviderId::new("local").unwrap(), request("durable"))
        .await
        .unwrap();
    let run = workspace
        .start_with(run_request(
            "stable-run",
            &["sleep-print", "0.4", "started", "completed"],
        ))
        .await
        .unwrap();
    let reference = run.reference();
    drop(run);
    drop(workspace);
    drop(first);
    let second = sdk(
        temp.path(),
        Arc::new(FileStateRepository::new(temp.path().join("state")).unwrap()),
    );
    let reconnected = second.workspace(&reference.workspace.id).await.unwrap();
    assert!(matches!(
        reconnected
            .run(reference.id.clone())
            .wait(Duration::from_secs(10))
            .await
            .unwrap(),
        RunState::Exited { exit_code: 0 }
    ));
    let output = reconnected
        .run(reference.id)
        .output(OutputCursor::default(), 4096)
        .await
        .unwrap();
    assert!(output.stdout.contains("completed"));
    let reopened = second
        .open(&ProviderId::new("local").unwrap(), request("durable"))
        .await
        .unwrap();
    assert_eq!(reopened.descriptor().root, reconnected.descriptor().root);
    let mut changed = request("durable");
    changed.config = ProviderOptions::from_options(&LocalOpenOptions {
        attach: Some(temp.path().to_path_buf()),
    })
    .unwrap();
    assert!(matches!(
        second
            .open(&ProviderId::new("local").unwrap(), changed)
            .await,
        Err(Error::Conflict(_))
    ));
}

#[tokio::test]
async fn launch_retry_does_not_repeat_side_effect_and_rejects_rebound_intent() {
    let temp = TempDir::new().unwrap();
    let repo = Arc::new(FileStateRepository::new(temp.path().join("state")).unwrap());
    let sdk = sdk(temp.path(), repo);
    let workspace = sdk
        .open(&ProviderId::new("local").unwrap(), request("launch"))
        .await
        .unwrap();
    let command = &["increment", "count", "1"];
    workspace
        .start_with(run_request("same", command))
        .await
        .unwrap()
        .wait(Duration::from_secs(10))
        .await
        .unwrap();
    workspace
        .start_with(run_request("same", command))
        .await
        .unwrap()
        .wait(Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(Path::new(&workspace.descriptor().root).join("count")).unwrap(),
        "1"
    );
    assert!(matches!(
        workspace
            .start_with(run_request("same", &["echo", "other"]))
            .await,
        Err(Error::Conflict(_))
    ));
}

#[tokio::test]
async fn sync_stages_merge_preserves_secrets_and_applies_only_with_fingerprint() {
    let temp = TempDir::new().unwrap();
    let local = temp.path().join("project");
    fs::create_dir(&local).unwrap();
    fs::write(local.join("local.txt"), "base local\n").unwrap();
    fs::write(local.join("remote.txt"), "base remote\n").unwrap();
    fs::write(local.join(".env"), "NEVER_COPY=private").unwrap();
    let repo = Arc::new(FileStateRepository::new(temp.path().join("state")).unwrap());
    let sdk = sdk(temp.path(), repo.clone());
    let target = sdk
        .open(&ProviderId::new("local").unwrap(), request("sync"))
        .await
        .unwrap();
    let engine = Arc::new(GitCheckpointEngine::new("git", temp.path().join("snapshots")).unwrap());
    let sync = SyncService::new(engine, repo);
    let id = OperationId::new("sync-test").unwrap();
    sync.initialize(id.clone(), &local, &target, true)
        .await
        .unwrap();
    let remote = PathBuf::from(target.descriptor().root);
    assert!(!remote.join(".env").exists());
    fs::write(local.join("local.txt"), "local change\n").unwrap();
    fs::write(remote.join("remote.txt"), "remote change\n").unwrap();
    let review = sync.sync_back(&id, &target, true).await.unwrap();
    assert_eq!(review.status, ReconciliationStatus::StagedForReview);
    assert!(review.conflicts.is_empty());
    assert_eq!(
        fs::read_to_string(local.join("remote.txt")).unwrap(),
        "base remote\n"
    );
    assert!(sync
        .apply_sync(
            &id,
            &review,
            ApplyOptions {
                quiescent: false,
                expected_fingerprint: review.expected_fingerprint.clone()
            }
        )
        .await
        .is_err());
    sync.apply_sync(
        &id,
        &review,
        ApplyOptions {
            quiescent: true,
            expected_fingerprint: review.expected_fingerprint.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        fs::read_to_string(local.join("remote.txt")).unwrap(),
        "remote change\n"
    );
    assert_eq!(
        fs::read_to_string(local.join("local.txt")).unwrap(),
        "local change\n"
    );
    assert_eq!(
        fs::read_to_string(local.join(".env")).unwrap(),
        "NEVER_COPY=private"
    );
    // Reopening an initialized session never rewinds remote edits.
    sync.initialize(id.clone(), &local, &target, true)
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(remote.join("remote.txt")).unwrap(),
        "remote change\n"
    );
    let second = sync.sync_back(&id, &target, true).await.unwrap();
    assert!(second.conflicts.is_empty());
}

#[tokio::test]
async fn handoff_refuses_active_source_and_resumes_once_across_controller_replacement() {
    let temp = TempDir::new().unwrap();
    let repo = Arc::new(FileStateRepository::new(temp.path().join("state")).unwrap());
    let sdk = sdk(temp.path(), repo.clone());
    let provider = ProviderId::new("local").unwrap();
    let source = sdk.open(&provider, request("source")).await.unwrap();
    let destination = sdk.open(&provider, request("destination")).await.unwrap();
    let active = source
        .start(CommandSpec::new(support::workload_program()).args(["sleep", "30"]))
        .await
        .unwrap();
    let engine = Arc::new(GitCheckpointEngine::new("git", temp.path().join("snapshots")).unwrap());
    let sync = SyncService::new(engine.clone(), repo.clone());
    let command = CommandSpec::new(support::workload_program()).args(["increment", "counter", "2"]);
    assert!(matches!(
        sync.handoff(
            OperationId::new("rejected").unwrap(),
            &source,
            &active,
            &destination,
            command.clone().into(),
            true
        )
        .await,
        Err(Error::Conflict(_))
    ));
    active.cancel().await.unwrap();
    let run = source
        .start(CommandSpec::new(support::workload_program()).args(["writefile", "counter", "3"]))
        .await
        .unwrap();
    run.wait(Duration::from_secs(10)).await.unwrap();
    let id = OperationId::new("handoff-once").unwrap();
    let receipt = sync
        .handoff(
            id.clone(),
            &source,
            &run,
            &destination,
            command.clone().into(),
            true,
        )
        .await
        .unwrap();
    destination
        .run(receipt.destination_run.clone().unwrap().id)
        .wait(Duration::from_secs(10))
        .await
        .unwrap();
    drop(sync);
    let restarted = SyncService::new(
        engine,
        Arc::new(FileStateRepository::new(temp.path().join("state")).unwrap()),
    );
    let mut changed_input = ExecutionRequest::from(command.clone());
    changed_input.private_stdin = Some(SecretInput::new(b"changed native input".to_vec()));
    assert!(matches!(
        restarted
            .handoff(id.clone(), &source, &run, &destination, changed_input, true)
            .await,
        Err(Error::Conflict(_))
    ));
    let repeated = restarted
        .handoff(id, &source, &run, &destination, command.into(), true)
        .await
        .unwrap();
    assert_eq!(
        repeated.destination_run.unwrap().id,
        receipt.destination_run.unwrap().id
    );
    assert_eq!(
        fs::read_to_string(Path::new(&destination.descriptor().root).join("counter")).unwrap(),
        "5"
    );
    assert_eq!(
        fs::read_to_string(Path::new(&source.descriptor().root).join("counter")).unwrap(),
        "3"
    );
}

/// Fail only the selected handoff publication, leaving capture and other workflows intact.
struct HandoffReceiptFaultStore {
    inner: Arc<FileStateRepository>,
    operation: OperationId,
    phase: HandoffPhase,
    commit_before_error: bool,
    fail_next: AtomicBool,
    attempted: Mutex<Option<HandoffReceipt>>,
}
impl IWorkflowRepository for HandoffReceiptFaultStore {
    fn acquire(&self, id: &OperationId) -> Result<Box<dyn IWorkflowLease>> {
        IWorkflowRepository::acquire(&*self.inner, id)
    }
    fn load(&self, id: &OperationId) -> Result<Option<WorkflowRecord>> {
        IWorkflowRepository::load(&*self.inner, id)
    }
    fn save(&self, id: &OperationId, value: &WorkflowRecord) -> Result<()> {
        if let WorkflowRecord::Handoff(receipt) = value {
            if id == &self.operation
                && std::mem::discriminant(&receipt.phase) == std::mem::discriminant(&self.phase)
                && self.fail_next.swap(false, Ordering::SeqCst)
            {
                *self.attempted.lock().unwrap() = Some(receipt.clone());
                if self.commit_before_error {
                    IWorkflowRepository::save(&*self.inner, id, value)?;
                }
                return Err(Error::State("injected handoff receipt failure".into()));
            }
        }
        IWorkflowRepository::save(&*self.inner, id, value)
    }
}

#[tokio::test]
async fn handoff_recovers_restored_receipt_failure_before_destination_launch() {
    handoff_recovers_phase_failure(HandoffPhase::Restored, false).await;
}

#[tokio::test]
async fn handoff_recovers_started_receipt_failure_without_relaunching_destination() {
    handoff_recovers_phase_failure(HandoffPhase::Started, false).await;
}

#[tokio::test]
async fn handoff_recovers_started_acknowledgement_loss_without_relaunching_destination() {
    handoff_recovers_phase_failure(HandoffPhase::Started, true).await;
}

async fn handoff_recovers_phase_failure(phase: HandoffPhase, commit_before_error: bool) {
    let temporary = TempDir::new().unwrap();
    let state = temporary.path().join("state");
    let repository = Arc::new(FileStateRepository::new(&state).unwrap());
    let controller = sdk(temporary.path(), repository.clone());
    let provider = ProviderId::new("local").unwrap();
    let source = controller.open(&provider, request("source")).await.unwrap();
    let destination = controller
        .open(&provider, request("destination"))
        .await
        .unwrap();
    let source_run = source
        .start(CommandSpec::new(support::workload_program()).args(["writefile", "counter", "3"]))
        .await
        .unwrap();
    assert_eq!(
        source_run.wait(Duration::from_secs(10)).await.unwrap(),
        RunState::Exited { exit_code: 0 }
    );
    let operation = OperationId::new("handoff-save-boundary").unwrap();
    let source_reference = source.reference();
    let destination_reference = destination.reference();
    let source_run_reference = source_run.reference();
    let engine =
        Arc::new(GitCheckpointEngine::new("git", temporary.path().join("snapshots")).unwrap());
    let receipts = Arc::new(HandoffReceiptFaultStore {
        inner: repository.clone(),
        operation: operation.clone(),
        phase,
        commit_before_error,
        fail_next: AtomicBool::new(true),
        attempted: Mutex::new(None),
    });
    let sync = SyncService::new(engine.clone(), receipts.clone());
    let continuation =
        CommandSpec::new(support::workload_program()).args(["increment", "counter", "2"]);
    assert!(matches!(
        sync.handoff(
            operation.clone(),
            &source,
            &source_run,
            &destination,
            continuation.clone().into(),
            true,
        )
        .await,
        Err(Error::State(message)) if message == "injected handoff receipt failure"
    ));
    let attempted = receipts.attempted.lock().unwrap().clone().unwrap();
    let destination_root = PathBuf::from(destination.descriptor().root);
    let started = matches!(attempted.phase, HandoffPhase::Started);
    if started {
        let reference = attempted.destination_run.as_ref().unwrap();
        assert_eq!(reference.workspace, destination_reference);
        assert_eq!(
            destination
                .run(reference.id.clone())
                .wait(Duration::from_secs(10))
                .await
                .unwrap(),
            RunState::Exited { exit_code: 0 }
        );
    } else {
        assert!(matches!(attempted.phase, HandoffPhase::Restored));
        assert!(attempted.destination_run.is_none());
        let runs = Path::new(&destination.descriptor().control_root).join("runs");
        assert!(fs::read_dir(runs).unwrap().all(|entry| !entry
            .unwrap()
            .path()
            .join("claim.json")
            .exists()));
    }
    let before_retry = if started { "5" } else { "3" };
    assert_eq!(
        fs::read_to_string(destination_root.join("counter")).unwrap(),
        before_retry
    );
    drop(sync);
    drop(receipts);
    drop(repository);
    drop(controller);
    drop(source_run);
    drop(source);
    drop(destination);

    // Reopen the actual disk repository, rather than reusing the fault wrapper's memory.
    let reopened = Arc::new(FileStateRepository::new(&state).unwrap());
    let saved = IWorkflowRepository::load(&*reopened, &operation)
        .unwrap()
        .unwrap()
        .into_handoff()
        .unwrap();
    match (&attempted.phase, commit_before_error) {
        (HandoffPhase::Restored, false) => assert!(matches!(saved.phase, HandoffPhase::Captured)),
        (HandoffPhase::Started, false) => assert!(matches!(saved.phase, HandoffPhase::Restored)),
        (HandoffPhase::Started, true) => assert!(matches!(saved.phase, HandoffPhase::Started)),
        _ => panic!("unsupported receipt fault scenario"),
    }
    let restarted_controller = sdk(temporary.path(), reopened.clone());
    let source = restarted_controller
        .connect(&source_reference)
        .await
        .unwrap();
    let destination = restarted_controller
        .connect(&destination_reference)
        .await
        .unwrap();
    let source_run = source.run(source_run_reference.id);
    let restarted = SyncService::new(engine, reopened);
    let changed = CommandSpec::new(support::workload_program()).args([
        "writefile",
        "unexpected",
        "changed continuation ran",
    ]);
    assert!(matches!(
        restarted
            .handoff(
                operation.clone(),
                &source,
                &source_run,
                &destination,
                changed.into(),
                true,
            )
            .await,
        Err(Error::Conflict(_))
    ));
    assert!(!destination_root.join("unexpected").exists());
    assert_eq!(
        fs::read_to_string(destination_root.join("counter")).unwrap(),
        before_retry
    );
    let recovered = restarted
        .handoff(
            operation.clone(),
            &source,
            &source_run,
            &destination,
            continuation.clone().into(),
            true,
        )
        .await
        .unwrap();
    assert!(matches!(recovered.phase, HandoffPhase::Started));
    let recovered_reference = recovered.destination_run.unwrap();
    assert_eq!(recovered_reference.workspace, destination_reference);
    if let Some(reference) = attempted.destination_run {
        assert_eq!(recovered_reference.workspace, reference.workspace);
        assert_eq!(recovered_reference.id, reference.id);
    }
    assert_eq!(
        destination
            .run(recovered_reference.id.clone())
            .wait(Duration::from_secs(10))
            .await
            .unwrap(),
        RunState::Exited { exit_code: 0 }
    );
    let repeated = restarted
        .handoff(
            operation,
            &source,
            &source_run,
            &destination,
            continuation.into(),
            true,
        )
        .await
        .unwrap();
    assert!(matches!(repeated.phase, HandoffPhase::Started));
    let repeated_reference = repeated.destination_run.unwrap();
    assert_eq!(repeated_reference.workspace, recovered_reference.workspace);
    assert_eq!(repeated_reference.id, recovered_reference.id);
    assert_eq!(
        fs::read_to_string(destination_root.join("counter")).unwrap(),
        "5"
    );
    assert!(!destination_root.join("unexpected").exists());
    assert_eq!(
        fs::read_to_string(Path::new(&source.descriptor().root).join("counter")).unwrap(),
        "3"
    );
}

struct FailOnceWorkflowStore {
    inner: Arc<FileStateRepository>,
    fail_next: AtomicBool,
}
impl IWorkflowRepository for FailOnceWorkflowStore {
    fn acquire(&self, id: &OperationId) -> Result<Box<dyn IWorkflowLease>> {
        IWorkflowRepository::acquire(&*self.inner, id)
    }
    fn load(&self, id: &OperationId) -> Result<Option<WorkflowRecord>> {
        IWorkflowRepository::load(&*self.inner, id)
    }
    fn save(&self, id: &OperationId, value: &WorkflowRecord) -> Result<()> {
        if self.fail_next.swap(false, Ordering::SeqCst) {
            return Err(Error::State("injected receipt save failure".into()));
        }
        IWorkflowRepository::save(&*self.inner, id, value)
    }
}
#[tokio::test]
async fn sync_recovers_receipt_failure_after_apply_without_overwriting_later_edits() {
    let temporary = TempDir::new().unwrap();
    let project = temporary.path().join("project");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("file.txt"), "base\n").unwrap();
    let repo = Arc::new(FileStateRepository::new(temporary.path().join("state")).unwrap());
    let sdk = sdk(temporary.path(), repo.clone());
    let workspace = sdk
        .open(
            &ProviderId::new("local").unwrap(),
            request("receipt-recovery"),
        )
        .await
        .unwrap();
    let engine =
        Arc::new(GitCheckpointEngine::new("git", temporary.path().join("checkpoints")).unwrap());
    let receipts = Arc::new(FailOnceWorkflowStore {
        inner: repo,
        fail_next: AtomicBool::new(false),
    });
    let sync = SyncService::new(engine, receipts.clone());
    let id = OperationId::new("recovery-sync").unwrap();
    sync.initialize(id.clone(), &project, &workspace, true)
        .await
        .unwrap();
    fs::write(
        Path::new(&workspace.descriptor().root).join("file.txt"),
        "remote\n",
    )
    .unwrap();
    let review = sync.sync_back(&id, &workspace, true).await.unwrap();
    let options = ApplyOptions {
        quiescent: true,
        expected_fingerprint: review.expected_fingerprint.clone(),
    };
    receipts.fail_next.store(true, Ordering::SeqCst);
    assert!(sync
        .apply_sync(&id, &review, options.clone())
        .await
        .is_err());
    assert_eq!(
        fs::read_to_string(project.join("file.txt")).unwrap(),
        "remote\n"
    );
    fs::write(project.join("file.txt"), "editor change after apply\n").unwrap();
    let recovered = sync
        .apply_sync(&id, &review, options.clone())
        .await
        .unwrap();
    assert_eq!(sync.session(&id).unwrap().baseline, review.remote);
    assert_eq!(
        fs::read_to_string(project.join("file.txt")).unwrap(),
        "editor change after apply\n"
    );
    assert_eq!(
        sync.apply_sync(&id, &review, options)
            .await
            .unwrap()
            .recovery,
        recovered.recovery
    );
}

#[tokio::test]
async fn caller_wait_timeout_does_not_cancel_the_detached_run() {
    let temporary = TempDir::new().unwrap();
    let repo = Arc::new(FileStateRepository::new(temporary.path().join("state")).unwrap());
    let sdk = sdk(temporary.path(), repo);
    let workspace = sdk
        .open(&ProviderId::new("local").unwrap(), request("wait-timeout"))
        .await
        .unwrap();
    let run = workspace
        .start(CommandSpec::new(support::workload_program()).args(["sleep", "0.5", "survived"]))
        .await
        .unwrap();
    assert!(matches!(
        run.wait(Duration::MAX).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        run.wait(Duration::from_millis(10)).await,
        Err(Error::Timeout(_))
    ));
    assert!(matches!(
        run.wait(Duration::from_secs(10)).await.unwrap(),
        RunState::Exited { exit_code: 0 }
    ));
    assert!(run
        .output(OutputCursor::default(), 4096)
        .await
        .unwrap()
        .stdout
        .contains("survived"));
}

#[tokio::test]
async fn next_sync_recovers_previous_apply_before_processing_a_remote_revert() {
    let temporary = TempDir::new().unwrap();
    let project = temporary.path().join("project");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("file.txt"), "old\n").unwrap();
    let repo = Arc::new(FileStateRepository::new(temporary.path().join("state")).unwrap());
    let sdk = sdk(temporary.path(), repo.clone());
    let target = sdk
        .open(&ProviderId::new("local").unwrap(), request("remote-revert"))
        .await
        .unwrap();
    let engine =
        Arc::new(GitCheckpointEngine::new("git", temporary.path().join("snapshots")).unwrap());
    let receipts = Arc::new(FailOnceWorkflowStore {
        inner: repo,
        fail_next: AtomicBool::new(false),
    });
    let sync = SyncService::new(engine, receipts.clone());
    let id = OperationId::new("revert-session").unwrap();
    sync.initialize(id.clone(), &project, &target, true)
        .await
        .unwrap();
    let remote = PathBuf::from(target.descriptor().root);
    fs::write(remote.join("file.txt"), "new\n").unwrap();
    let first = sync.sync_back(&id, &target, true).await.unwrap();
    receipts.fail_next.store(true, Ordering::SeqCst);
    assert!(sync
        .apply_sync(
            &id,
            &first,
            ApplyOptions {
                quiescent: true,
                expected_fingerprint: first.expected_fingerprint.clone()
            }
        )
        .await
        .is_err());
    fs::write(remote.join("file.txt"), "old\n").unwrap();
    let second = sync.sync_back(&id, &target, true).await.unwrap();
    assert_eq!(second.base, first.remote);
    assert!(second.conflicts.is_empty());
    sync.apply_sync(
        &id,
        &second,
        ApplyOptions {
            quiescent: true,
            expected_fingerprint: second.expected_fingerprint.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        fs::read_to_string(project.join("file.txt")).unwrap(),
        "old\n"
    );
}
