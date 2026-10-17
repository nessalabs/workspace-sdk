//! In-process caller loss must not drop effect ownership while a real provider is active.
//! This does not simulate runtime/process death or qualify remote host failover.
mod support;

use async_trait::async_trait;
use std::{
    fs,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{Notify, Semaphore};
use workspace_sdk::{
    checkpoint::{CaptureOptions, GitCheckpointEngine, ICheckpointEngine},
    infrastructure::FileStateRepository,
    ports::IProvider,
    providers::{LocalConfig, LocalProvider},
    *,
};

/// Gate only the first open, after a native target really exists. Every provider
/// operation delegates to the real adapter; the test never synthesizes a run.
struct GatedProvider {
    inner: LocalProvider,
    opens: AtomicUsize,
    entered: Notify,
    release: Semaphore,
}

#[async_trait]
impl IProvider for GatedProvider {
    fn id(&self) -> ProviderId {
        self.inner.id()
    }

    async fn open(&self, request: OpenRequest) -> Result<Arc<dyn IBackend>> {
        let backend = self.inner.open(request).await?;
        if self.opens.fetch_add(1, Ordering::SeqCst) == 0 {
            self.entered.notify_one();
            self.release.acquire().await.unwrap().forget();
        }
        Ok(backend)
    }

    async fn connect(&self, reference: &WorkspaceRef) -> Result<Arc<dyn IBackend>> {
        self.inner.connect(reference).await
    }
}

fn request() -> RecoveryRequest {
    RecoveryRequest {
        request_id: OperationId::new("stable-switch").unwrap(),
        expected_previous_epoch: 0,
        available: vec![ProviderId::new("local").unwrap()],
        continuation: CommandSpec::new(support::workload_program())
            .args(["increment", "counter", "2"])
            .into(),
        safety: RecoverySafety::ConfirmedStopped,
    }
}

#[tokio::test]
async fn caller_abort_keeps_effect_lease_until_owned_recovery_finishes() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("seed-project");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("counter"), "3").unwrap();
    let engine =
        Arc::new(GitCheckpointEngine::new("git", temporary.path().join("artifacts")).unwrap());
    let seed = engine
        .capture(&source, CaptureOptions::default())
        .unwrap()
        .reference()
        .clone();
    let repository =
        Arc::new(FileStateRepository::new(temporary.path().join("authority")).unwrap());
    let mut config = LocalConfig::trusted_host(temporary.path().join("native-targets"));
    config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
    let provider = Arc::new(GatedProvider {
        inner: LocalProvider::new(config).unwrap(),
        opens: AtomicUsize::new(0),
        entered: Notify::new(),
        release: Semaphore::new(0),
    });
    let sdk = Arc::new(
        WorkspaceSdk::builder(repository.clone())
            .provider(provider.clone())
            .unwrap()
            .build()
            .unwrap(),
    );
    let recovery = Arc::new(RecoveryService::new(
        sdk.clone(),
        Arc::new(SyncService::new(engine, repository.clone())),
        repository,
    ));
    let id = OperationId::new("caller-loss").unwrap();
    recovery
        .create(
            id.clone(),
            vec![RecoveryCandidate {
                provider: ProviderId::new("local").unwrap(),
                config: ProviderOptions::default(),
                resources: ResourceRequirements::default(),
            }],
            seed,
        )
        .await
        .unwrap();

    let caller = tokio::spawn({
        let recovery = recovery.clone();
        let id = id.clone();
        async move { recovery.recover(&id, request()).await }
    });
    tokio::time::timeout(Duration::from_secs(10), provider.entered.notified())
        .await
        .expect("real native open did not reach the gate");
    let pending = recovery.session(&id).unwrap();
    assert!(matches!(
        pending.current_attempt.as_ref().unwrap().phase,
        RecoveryPhase::Preparing
    ));
    assert_eq!(provider.opens.load(Ordering::SeqCst), 1);
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());

    // Save the outcome, then release before asserting so a deliberate regression
    // failure cannot strand the owned task at the test gate.
    let competing =
        tokio::time::timeout(Duration::from_secs(10), recovery.recover(&id, request())).await;
    provider.release.add_permits(1);
    assert!(matches!(competing, Ok(Err(Error::Conflict(_)))),
        "a second controller acquired the effect lease while native open was still active: {competing:?}");

    let completed = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let session = recovery.session(&id).unwrap();
            if matches!(
                session.current_attempt.as_ref().unwrap().phase,
                RecoveryPhase::Started { .. }
            ) {
                break session;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("owned recovery did not finish after caller abort and gate release");
    let RecoveryPhase::Started { run, .. } = &completed.current_attempt.as_ref().unwrap().phase
    else {
        unreachable!()
    };
    let workspace = sdk.connect(&run.workspace).await.unwrap();
    assert_eq!(
        workspace
            .run(run.id.clone())
            .wait(Duration::from_secs(10))
            .await
            .unwrap(),
        RunState::Exited { exit_code: 0 }
    );
    let counter = std::path::Path::new(&workspace.descriptor().root).join("counter");
    assert_eq!(fs::read_to_string(&counter).unwrap(), "5");
    assert_eq!(fs::read_to_string(source.join("counter")).unwrap(), "3");

    let retried = recovery.recover(&id, request()).await.unwrap();
    let RecoveryPhase::Started {
        run: retried_run, ..
    } = &retried.current_attempt.as_ref().unwrap().phase
    else {
        panic!("exact retry lost accepted run identity")
    };
    assert_eq!(retried_run.workspace, run.workspace);
    assert_eq!(retried_run.id, run.id);
    assert_eq!(provider.opens.load(Ordering::SeqCst), 1);
    assert_eq!(fs::read_to_string(counter).unwrap(), "5");
}

/// Hold the acknowledgement after a real supervisor has written staging bytes.
/// Protocol inspection controls timing only; all effects and responses are real.
struct GatedWriteRunner {
    writes: AtomicUsize,
    imports: AtomicUsize,
    cleanups: AtomicUsize,
    launches: AtomicUsize,
    staged_file: std::sync::Mutex<Option<std::path::PathBuf>>,
    entered: Notify,
    release: Semaphore,
}

#[async_trait]
impl workspace_sdk::providers::ICommandRunner for GatedWriteRunner {
    async fn run(&self, command: workspace_sdk::providers::NativeCommand) -> Result<ExecResult> {
        use workspace_sdk::supervisor::protocol::{SupervisorOperation, SupervisorRequest};
        let request = serde_json::from_slice::<SupervisorRequest>(&command.stdin).ok();
        let outcome = workspace_sdk::providers::ICommandRunner::run(
            &workspace_sdk::providers::SystemCommandRunner,
            command,
        )
        .await?;
        if outcome.success() {
            if let Some(request) = request {
                match request.operation {
                    SupervisorOperation::WriteFile { path, .. }
                        if path.as_str().starts_with(".workspace-sdk/transfers/") =>
                    {
                        if self.writes.fetch_add(1, Ordering::SeqCst) == 0 {
                            let staged =
                                std::path::Path::new(&request.context.root).join(path.as_str());
                            assert!(fs::metadata(&staged).unwrap().len() > 0);
                            *self.staged_file.lock().unwrap() = Some(staged);
                            self.entered.notify_one();
                            self.release.acquire().await.unwrap().forget();
                        }
                    }
                    SupervisorOperation::ImportTree { .. } => {
                        self.imports.fetch_add(1, Ordering::SeqCst);
                    }
                    SupervisorOperation::RemoveTransfer { .. } => {
                        self.cleanups.fetch_add(1, Ordering::SeqCst);
                    }
                    SupervisorOperation::Start { .. } => {
                        self.launches.fetch_add(1, Ordering::SeqCst);
                    }
                    _ => {}
                }
            }
        }
        Ok(outcome)
    }
}

#[tokio::test]
async fn caller_abort_after_actual_staging_write_finishes_restore_cleanup_and_one_launch() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("seed-project");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("counter"), "3").unwrap();
    // Multiple real archive chunks force continuation after the held first write.
    fs::write(source.join("payload"), vec![b'x'; 192 * 1024]).unwrap();
    let engine =
        Arc::new(GitCheckpointEngine::new("git", temporary.path().join("artifacts")).unwrap());
    let seed = engine
        .capture(&source, CaptureOptions::default())
        .unwrap()
        .reference()
        .clone();
    let repository =
        Arc::new(FileStateRepository::new(temporary.path().join("authority")).unwrap());
    let runner = Arc::new(GatedWriteRunner {
        writes: AtomicUsize::new(0),
        imports: AtomicUsize::new(0),
        cleanups: AtomicUsize::new(0),
        launches: AtomicUsize::new(0),
        staged_file: std::sync::Mutex::new(None),
        entered: Notify::new(),
        release: Semaphore::new(0),
    });
    let mut config = LocalConfig::trusted_host(temporary.path().join("native-targets"));
    config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
    let sdk = Arc::new(
        WorkspaceSdk::builder(repository.clone())
            .provider(Arc::new(
                LocalProvider::with_runner(config, runner.clone()).unwrap(),
            ))
            .unwrap()
            .build()
            .unwrap(),
    );
    let recovery = Arc::new(RecoveryService::new(
        sdk.clone(),
        Arc::new(SyncService::new(engine, repository.clone())),
        repository,
    ));
    let id = OperationId::new("transfer-caller-loss").unwrap();
    recovery
        .create(
            id.clone(),
            vec![RecoveryCandidate {
                provider: ProviderId::new("local").unwrap(),
                config: ProviderOptions::default(),
                resources: ResourceRequirements::default(),
            }],
            seed,
        )
        .await
        .unwrap();
    let caller = tokio::spawn({
        let recovery = recovery.clone();
        let id = id.clone();
        async move { recovery.recover(&id, request()).await }
    });
    tokio::time::timeout(Duration::from_secs(10), runner.entered.notified())
        .await
        .expect("real supervisor staging write did not reach the acknowledgement gate");
    let staged = runner.staged_file.lock().unwrap().clone().unwrap();
    assert!(staged.is_file());
    assert_eq!(runner.imports.load(Ordering::SeqCst), 0);
    assert_eq!(runner.cleanups.load(Ordering::SeqCst), 0);
    assert_eq!(runner.launches.load(Ordering::SeqCst), 0);
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    let competing =
        tokio::time::timeout(Duration::from_secs(10), recovery.recover(&id, request())).await;
    runner.release.add_permits(1);
    assert!(
        matches!(competing, Ok(Err(Error::Conflict(_)))),
        "caller loss dropped transfer effect ownership: {competing:?}"
    );
    let completed = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let session = recovery.session(&id).unwrap();
            if matches!(
                session.current_attempt.as_ref().unwrap().phase,
                RecoveryPhase::Started { .. }
            ) {
                break session;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("owned transfer did not finish after caller abort");
    assert!(runner.writes.load(Ordering::SeqCst) > 1);
    assert_eq!(runner.imports.load(Ordering::SeqCst), 1);
    assert_eq!(runner.cleanups.load(Ordering::SeqCst), 1);
    assert!(
        !staged.exists(),
        "completed restoration left its staging archive behind"
    );
    assert!(
        !staged.parent().unwrap().exists(),
        "completed restoration left its transfer directory behind"
    );
    let RecoveryPhase::Started { run, .. } = &completed.current_attempt.as_ref().unwrap().phase
    else {
        unreachable!()
    };
    let workspace = sdk.connect(&run.workspace).await.unwrap();
    assert_eq!(
        workspace
            .run(run.id.clone())
            .wait(Duration::from_secs(10))
            .await
            .unwrap(),
        RunState::Exited { exit_code: 0 }
    );
    let root = std::path::PathBuf::from(workspace.descriptor().root);
    assert_eq!(fs::read_to_string(root.join("counter")).unwrap(), "5");
    assert_eq!(
        fs::read(root.join("payload")).unwrap(),
        fs::read(source.join("payload")).unwrap()
    );
    let retried = recovery.recover(&id, request()).await.unwrap();
    let RecoveryPhase::Started { run: retry, .. } =
        &retried.current_attempt.as_ref().unwrap().phase
    else {
        panic!("retry lost started identity")
    };
    assert_eq!(retry.workspace, run.workspace);
    assert_eq!(retry.id, run.id);
    assert_eq!(runner.launches.load(Ordering::SeqCst), 1);
    assert_eq!(fs::read_to_string(root.join("counter")).unwrap(), "5");
    assert_eq!(fs::read_to_string(source.join("counter")).unwrap(), "3");
}
