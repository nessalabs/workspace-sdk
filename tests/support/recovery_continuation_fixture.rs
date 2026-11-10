//! Real native namespaces and a narrow recovery-receipt fault boundary.
use async_trait::async_trait;
use std::{
    fs::{self, OpenOptions},
    future::Future,
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use workspace_sdk::{
    checkpoint::{CaptureOptions, GitCheckpointEngine, ICheckpointEngine},
    infrastructure::FileStateRepository,
    ports::{IRecoveryRepository, IWorkflowLease},
    providers::{LocalConfig, LocalProvider},
    *,
};

pub fn op(value: &str) -> OperationId {
    OperationId::new(value).unwrap()
}
pub fn host(value: &str) -> ProviderId {
    ProviderId::new(value).unwrap()
}
pub fn increment() -> ExecutionRequest {
    CommandSpec::new(super::support::workload_program())
        .args(["increment", "counter", "1"])
        .into()
}
pub fn fresh(name: &str, previous: u64, where_: &str) -> RecoveryRequest {
    RecoveryRequest {
        request_id: op(name),
        expected_previous_epoch: previous,
        available: vec![host(where_)],
        continuation: increment(),
        safety: RecoverySafety::ConfirmedStopped,
    }
}
pub fn retained(name: &str, session: &RecoverySession) -> RecoveryContinuationRequest {
    RecoveryContinuationRequest {
        request_id: op(name),
        expected_previous_epoch: session.current_attempt.as_ref().unwrap().epoch,
        expected_committed: session.committed.clone(),
        continuation: increment(),
        quiescent: true,
    }
}
pub fn started(session: &RecoverySession) -> RunRef {
    match &session.current_attempt.as_ref().unwrap().phase {
        RecoveryPhase::Started { run, .. } => run.clone(),
        phase => panic!("not accepted {phase:?}"),
    }
}
pub fn same(left: &RunRef, right: &RunRef) {
    assert_eq!(left.workspace, right.workspace);
    assert_eq!(left.id, right.id);
}
pub fn snapshot(session: &RecoverySession) -> serde_json::Value {
    serde_json::to_value(session).unwrap()
}

struct ControlledProvider {
    inner: LocalProvider,
    online: Arc<AtomicBool>,
}
#[async_trait]
impl IProvider for ControlledProvider {
    fn id(&self) -> ProviderId {
        self.inner.id()
    }
    async fn open(&self, request: OpenRequest) -> Result<Arc<dyn IBackend>> {
        self.inner.open(request).await
    }
    async fn connect(&self, reference: &WorkspaceRef) -> Result<Arc<dyn IBackend>> {
        if !self.online.load(Ordering::Acquire) {
            return Err(Error::Provider {
                provider: "native-fixture".into(),
                message: "modeled unavailable transport".into(),
            });
        }
        self.inner.connect(reference).await
    }
}
pub struct ReceiptFault {
    pub inner: Arc<FileStateRepository>,
    pub fail: AtomicBool,
    pub commit_before_error: AtomicBool,
    pub fail_restored: AtomicBool,
    pub attempted: Mutex<Option<RecoverySession>>,
    gate: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>,
}
impl ReceiptFault {
    pub fn gate(&self) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (entered_tx, entered) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        *self.gate.lock().unwrap() = Some((entered_tx, release_rx));
        (entered, release)
    }
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
        revision: Option<u64>,
        replacement: &RecoverySession,
    ) -> Result<()> {
        let started = replacement.current_attempt.as_ref().is_some_and(|attempt| {
            attempt.epoch >= 2 && matches!(attempt.phase, RecoveryPhase::Started { .. })
        });
        let restored = replacement.current_attempt.as_ref().is_some_and(|attempt| {
            attempt.retained.is_some() && matches!(attempt.phase, RecoveryPhase::Restored { .. })
        });
        if started {
            if let Some((entered, release)) = self.gate.lock().unwrap().take() {
                let _ = entered.send(());
                release
                    .recv_timeout(Duration::from_secs(10))
                    .map_err(|_| Error::State("fixture Started gate timed out".into()))?;
            }
        }
        let fail_restored = self.fail_restored.load(Ordering::Acquire);
        if (if fail_restored { restored } else { started })
            && self.fail.swap(false, Ordering::AcqRel)
        {
            *self.attempted.lock().unwrap() = Some(replacement.clone());
            if self.commit_before_error.load(Ordering::Acquire) {
                self.inner
                    .compare_exchange_recovery(id, revision, replacement)?;
            }
            return Err(Error::State(
                if fail_restored {
                    "injected retained Restored receipt failure"
                } else {
                    "injected retained Started receipt failure"
                }
                .into(),
            ));
        }
        self.inner
            .compare_exchange_recovery(id, revision, replacement)
    }
}
pub struct Fixture {
    pub root: PathBuf,
    pub sdk: Arc<WorkspaceSdk>,
    pub recovery: RecoveryService,
    pub faults: Arc<ReceiptFault>,
    pub online: Arc<AtomicBool>,
    pub id: OperationId,
    engine: Arc<GitCheckpointEngine>,
}
impl Fixture {
    pub async fn new(name: &str) -> Arc<Self> {
        let root = tempfile::tempdir().unwrap().keep().canonicalize().unwrap();
        eprintln!(
            "retained-continuation fixture retained until native ownership verified: {}",
            root.display()
        );
        let repository = Arc::new(FileStateRepository::new(root.join("authority")).unwrap());
        let online = Arc::new(AtomicBool::new(true));
        let mut builder = WorkspaceSdk::builder(repository.clone());
        for name in ["laptop", "devbox"] {
            let mut config = LocalConfig::trusted_host(root.join("workers").join(name));
            config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
            let provider = ControlledProvider {
                inner: LocalProvider::new(config).unwrap().with_id(host(name)),
                online: online.clone(),
            };
            builder = builder.provider(Arc::new(provider)).unwrap();
        }
        let sdk = Arc::new(builder.build().unwrap());
        let engine = Arc::new(GitCheckpointEngine::new("git", root.join("artifacts")).unwrap());
        let seed = root.join("seed");
        fs::create_dir(&seed).unwrap();
        fs::write(seed.join("counter"), "0").unwrap();
        let checkpoint = engine.capture(&seed, CaptureOptions::default()).unwrap();
        let faults = Arc::new(ReceiptFault {
            inner: repository.clone(),
            fail: AtomicBool::new(false),
            commit_before_error: AtomicBool::new(false),
            fail_restored: AtomicBool::new(false),
            attempted: Mutex::new(None),
            gate: Mutex::new(None),
        });
        let recovery = RecoveryService::new(
            sdk.clone(),
            Arc::new(SyncService::new(engine.clone(), repository)),
            faults.clone(),
        );
        let id = op(name);
        recovery
            .create(
                id.clone(),
                ["laptop", "devbox"]
                    .into_iter()
                    .map(|name| RecoveryCandidate {
                        provider: host(name),
                        config: ProviderOptions::default(),
                        resources: ResourceRequirements::default(),
                    })
                    .collect(),
                checkpoint.reference().clone(),
            )
            .await
            .unwrap();
        Arc::new(Self {
            root,
            sdk,
            recovery,
            faults,
            online,
            id,
            engine,
        })
    }
    pub fn session(&self) -> RecoverySession {
        self.recovery.session(&self.id).unwrap()
    }
    pub fn reopen(&self) -> RecoveryService {
        let repository = Arc::new(FileStateRepository::new(self.root.join("authority")).unwrap());
        RecoveryService::new(
            self.sdk.clone(),
            Arc::new(SyncService::new(self.engine.clone(), repository.clone())),
            repository,
        )
    }
    pub async fn initial(&self) -> RecoverySession {
        self.recovery
            .recover(&self.id, fresh("initial", 0, "laptop"))
            .await
            .unwrap()
    }
    pub async fn root_of(&self, session: &RecoverySession) -> PathBuf {
        self.sdk
            .connect(&started(session).workspace)
            .await
            .unwrap()
            .descriptor()
            .root
            .into()
    }
    pub async fn wait(&self, session: &RecoverySession) -> RunState {
        let run = started(session);
        self.sdk
            .connect(&run.workspace)
            .await
            .unwrap()
            .run(run.id)
            .wait(Duration::from_secs(10))
            .await
            .unwrap()
    }
    pub async fn acknowledged(&self) -> RecoverySession {
        let session = self.initial().await;
        assert_eq!(self.wait(&session).await, RunState::Exited { exit_code: 0 });
        self.recovery.publish(&self.id, 1, true).await.unwrap()
    }
    pub async fn owner_released(&self, session: &RecoverySession) {
        let run = started(session);
        let descriptor = self.sdk.connect(&run.workspace).await.unwrap().descriptor();
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(
                PathBuf::from(descriptor.control_root)
                    .join("runs")
                    .join(run.id.as_str())
                    .join("active.lock"),
            )
            .unwrap();
        assert!(lock.metadata().unwrap().is_file());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match fs2::FileExt::try_lock_exclusive(&lock) {
                Ok(()) => break,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    tokio::time::sleep(Duration::from_millis(5)).await
                }
                Err(error) => panic!("native owner not released: {error}"),
            }
        }
        fs2::FileExt::unlock(&lock).unwrap();
    }
    pub async fn run_count(&self) -> usize {
        self.sdk
            .list()
            .unwrap()
            .iter()
            .map(|descriptor| {
                let runs = PathBuf::from(&descriptor.control_root).join("runs");
                if runs.exists() {
                    fs::read_dir(runs).unwrap().count()
                } else {
                    0
                }
            })
            .sum()
    }
    async fn cleanup(&self) -> std::result::Result<(), String> {
        self.online.store(true, Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(25);
        let _lease = loop {
            match self.faults.inner.acquire_recovery(&self.id) {
                Ok(lease) => break lease,
                Err(Error::Conflict(_)) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(5)).await
                }
                Err(error) => return Err(error.to_string()),
            }
        };
        let mut count = 0;
        for descriptor in self.sdk.list().map_err(|error| error.to_string())? {
            let control = PathBuf::from(&descriptor.control_root);
            if !control.starts_with(self.root.join("workers")) {
                return Err("control escaped native fixture".into());
            }
            let runs = control.join("runs");
            if !runs.exists() {
                continue;
            }
            for entry in fs::read_dir(runs).map_err(|error| error.to_string())? {
                count += 1;
                if count > 64 {
                    return Err("native inventory exceeds fixture bound".into());
                }
                let entry = entry.map_err(|error| error.to_string())?;
                if !entry
                    .file_type()
                    .map_err(|error| error.to_string())?
                    .is_dir()
                {
                    return Err("unexpected native entry".into());
                }
                let id = RunId::new(entry.file_name().to_str().ok_or("nonUTF8 native ID")?)
                    .map_err(|error| error.to_string())?;
                let workspace = self
                    .sdk
                    .connect(&descriptor.reference)
                    .await
                    .map_err(|error| error.to_string())?;
                let run = workspace.run(id);
                let state = run.state().await.map_err(|error| error.to_string())?;
                if matches!(state, RunState::Lost { .. }) {
                    return Err("native ownership remains Lost".into());
                }
                if !state.is_terminal() {
                    run.cancel().await.map_err(|error| error.to_string())?;
                    run.wait(Duration::from_secs(10))
                        .await
                        .map_err(|error| error.to_string())?;
                }
                let lock = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                    .open(entry.path().join("active.lock"))
                    .map_err(|error| error.to_string())?;
                if !lock
                    .metadata()
                    .map_err(|error| error.to_string())?
                    .is_file()
                {
                    return Err("native owner lock is not regular".into());
                }
                loop {
                    match fs2::FileExt::try_lock_exclusive(&lock) {
                        Ok(()) => break,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            tokio::time::sleep(Duration::from_millis(5)).await
                        }
                        Err(error) => return Err(error.to_string()),
                    }
                }
                if !run
                    .state()
                    .await
                    .map_err(|error| error.to_string())?
                    .is_terminal()
                {
                    return Err("native state changed during owner inspection".into());
                }
                fs2::FileExt::unlock(&lock).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }
}
pub async fn run_case<F, Fut>(name: &str, case: F)
where
    F: FnOnce(Arc<Fixture>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let fixture = Fixture::new(name).await;
    let owned = fixture.clone();
    let mut task = tokio::spawn(async move { case(owned).await });
    let failed = match tokio::time::timeout(Duration::from_secs(120), &mut task).await {
        Ok(result) => result.err().map(|error| error.to_string()),
        Err(_) => {
            task.abort();
            let _ = task.await;
            Some("case watchdog expired".into())
        }
    };
    let cleaned = tokio::time::timeout(Duration::from_secs(35), fixture.cleanup()).await;
    match (failed, cleaned) {
        (None, Ok(Ok(()))) => fs::remove_dir_all(&fixture.root).unwrap(),
        (failed, cleaned) => panic!(
            "retained native evidence {}: case={failed:?}; cleanup={cleaned:?}",
            fixture.root.display()
        ),
    }
}
