//! Each workspace has one saved machine record: pending before any effect,
//! then Ready or Refused, never back. A real local provider does the work; the
//! wrapper only chooses what one create or attach attempt reports.
use async_trait::async_trait;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tempfile::TempDir;
use tokio::sync::Notify;
use workspace_sdk::{
    infrastructure::FileStateRepository,
    ports::{IProvider, IWorkspaceRepository},
    providers::{LocalAttachOptions, LocalConfig, LocalProvider},
    *,
};

#[derive(Clone)]
enum Next {
    /// Delegate to the real provider.
    Real,
    /// Wait for the signal, then delegate to the real provider.
    RealAfter(Arc<Notify>),
    /// Fail with no outcome, as a lost reply or crash would.
    Lost,
    /// Refuse the target.
    Refuse,
}

struct Scripted {
    inner: LocalProvider,
    next: Mutex<Vec<Next>>,
    keys: Mutex<Vec<ProvisionKey>>,
    calls: Mutex<usize>,
}
impl Scripted {
    fn next(&self) -> Next {
        *self.calls.lock().unwrap() += 1;
        self.next.lock().unwrap().pop().unwrap_or(Next::Real)
    }
    fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}
fn lost() -> Error {
    Error::Provider {
        provider: "scripted".into(),
        message: "reply lost".into(),
    }
}
#[async_trait]
impl IProvider for Scripted {
    fn id(&self) -> ProviderId {
        self.inner.id()
    }
    fn check_create(&self, request: &CreateRequest) -> Result<()> {
        self.inner.check_create(request)
    }
    fn check_attach(&self, request: &AttachRequest) -> Result<()> {
        self.inner.check_attach(request)
    }
    async fn create(&self, request: CreateRequest) -> Result<Arc<dyn IBackend>> {
        self.keys.lock().unwrap().push(request.provision_key());
        match self.next() {
            Next::Real => self.inner.create(request).await,
            Next::RealAfter(signal) => {
                signal.notified().await;
                self.inner.create(request).await
            }
            Next::Lost => Err(lost()),
            Next::Refuse => Err(Error::Refused(Box::new(refusal()))),
        }
    }
    async fn attach(&self, request: AttachRequest) -> Result<Arc<dyn IBackend>> {
        match self.next() {
            Next::Real => self.inner.attach(request).await,
            Next::RealAfter(signal) => {
                signal.notified().await;
                self.inner.attach(request).await
            }
            Next::Lost => Err(lost()),
            Next::Refuse => Err(Error::Refused(Box::new(refusal()))),
        }
    }
    async fn connect(&self, reference: &WorkspaceRef) -> Result<Arc<dyn IBackend>> {
        self.inner.connect(reference).await
    }
}

fn refusal() -> Refusal {
    Refusal {
        kind: RefusalKind::EnvironmentMismatch,
        message: "wrong_version".into(),
        cleanup: RefusalCleanup::Removed,
        target: Some("vm-1".into()),
        admission: None,
    }
}

/// Script attempts in the order they will happen.
fn sdk(root: &Path, attempts: &[Next]) -> (Arc<WorkspaceSdk>, Arc<Scripted>) {
    let mut config = LocalConfig::trusted_host(root.join("targets"));
    config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
    let provider = Arc::new(Scripted {
        inner: LocalProvider::new(config).unwrap(),
        next: Mutex::new(attempts.iter().rev().cloned().collect()),
        keys: Mutex::new(Vec::new()),
        calls: Mutex::new(0),
    });
    let repository = Arc::new(FileStateRepository::new(root.join("state")).unwrap());
    let sdk = WorkspaceSdk::builder(repository)
        .provider(provider.clone())
        .unwrap()
        .build()
        .unwrap();
    (Arc::new(sdk), provider)
}
fn local() -> ProviderId {
    ProviderId::new("local").unwrap()
}
fn id(value: &str) -> WorkspaceId {
    WorkspaceId::new(value).unwrap()
}
fn project(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::create_dir_all(&path).unwrap();
    path
}
fn attach_request(workspace: &WorkspaceId, path: PathBuf) -> AttachRequest {
    AttachRequest::new(workspace.clone(), &LocalAttachOptions { path }).unwrap()
}
async fn until(condition: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn lost_create_stays_provisioning_and_a_restarted_retry_reuses_its_key() {
    let temp = TempDir::new().unwrap();
    let workspace = id("lost-reply");
    {
        let (sdk, _) = sdk(temp.path(), &[Next::Lost]);
        assert!(sdk
            .create(&local(), CreateRequest::new(workspace.clone()))
            .await
            .is_err());
        let record = sdk.record(&workspace).unwrap().unwrap();
        assert_eq!(
            record.state,
            MachineState::Provisioning {
                key: ProvisionKey::for_workspace(&workspace)
            }
        );
        assert!(sdk.workspace(&workspace).await.is_err());
    }
    // A new process continues from the saved record with the same key.
    let (sdk, provider) = sdk(temp.path(), &[Next::Real]);
    let created = sdk
        .create(&local(), CreateRequest::new(workspace.clone()))
        .await
        .unwrap();
    assert_eq!(
        provider.keys.lock().unwrap().as_slice(),
        [ProvisionKey::for_workspace(&workspace)]
    );
    let record = sdk.record(&workspace).unwrap().unwrap();
    assert_eq!(record.descriptor(), Some(&created.descriptor()));
    // Once Ready, a retry reconnects and never asks the provider again.
    sdk.create(&local(), CreateRequest::new(workspace.clone()))
        .await
        .unwrap();
    assert_eq!(provider.calls(), 1);
}

#[tokio::test]
async fn a_refusal_is_saved_and_final() {
    let temp = TempDir::new().unwrap();
    let workspace = id("refused");
    let (sdk, provider) = sdk(temp.path(), &[Next::Refuse, Next::Real]);
    let refused = sdk
        .create(&local(), CreateRequest::new(workspace.clone()))
        .await;
    assert!(matches!(refused, Err(Error::Refused(saved)) if *saved == refusal()));
    // The next attempt would succeed, but the saved refusal stands.
    assert!(matches!(
        sdk.create(&local(), CreateRequest::new(workspace.clone()))
            .await,
        Err(Error::Refused(_))
    ));
    assert_eq!(provider.calls(), 1);
    let revision = sdk.record(&workspace).unwrap().unwrap().revision;
    assert!(matches!(
        sdk.forget_unfinished(&workspace, revision),
        Err(Error::Conflict(_))
    ));
    assert!(sdk.list().unwrap().is_empty());
}

#[tokio::test]
async fn forgetting_an_unfinished_create_frees_the_id() {
    let temp = TempDir::new().unwrap();
    let workspace = id("forgotten");
    let (sdk, _) = sdk(temp.path(), &[Next::Lost, Next::Real]);
    assert!(sdk
        .create(&local(), CreateRequest::new(workspace.clone()))
        .await
        .is_err());
    let revision = sdk.record(&workspace).unwrap().unwrap().revision;
    // A caller that read another revision cannot forget it.
    assert!(matches!(
        sdk.forget_unfinished(&workspace, revision + 1),
        Err(Error::Conflict(_))
    ));
    sdk.forget_unfinished(&workspace, revision).unwrap();
    assert!(sdk.record(&workspace).unwrap().is_none());
    // The ID is free again, even for an attach.
    let attached = sdk
        .attach(
            &local(),
            attach_request(&workspace, project(temp.path(), "project")),
        )
        .await
        .unwrap();
    assert_eq!(attached.descriptor().ownership, Ownership::Attached);
    // A new record never reuses a revision the forgotten one had.
    assert!(sdk.record(&workspace).unwrap().unwrap().revision > revision);
    assert!(matches!(
        sdk.connect_owned(&attached.reference()).await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        sdk.owned_workspace(&workspace).await,
        Err(Error::Conflict(_))
    ));
}

#[tokio::test]
async fn a_request_the_provider_rejects_reserves_nothing() {
    let temp = TempDir::new().unwrap();
    let workspace = id("bad-options");
    let (sdk, provider) = sdk(temp.path(), &[]);
    let bad =
        CreateRequest::with_options(workspace.clone(), &serde_json::json!({"unexpected": true}))
            .unwrap();
    assert!(sdk.create(&local(), bad).await.is_err());
    let bad = AttachRequest::new(workspace.clone(), &serde_json::json!({"nope": 1})).unwrap();
    assert!(sdk.attach(&local(), bad).await.is_err());
    // A path that does not exist can never attach either.
    let missing = attach_request(&workspace, temp.path().join("typo"));
    assert!(sdk.attach(&local(), missing).await.is_err());
    assert!(sdk.record(&workspace).unwrap().is_none());
    assert_eq!(provider.calls(), 0);
}

#[tokio::test]
async fn create_and_attach_never_share_an_id() {
    let temp = TempDir::new().unwrap();
    let (sdk, _) = sdk(temp.path(), &[]);
    let created = id("created-first");
    sdk.create(&local(), CreateRequest::new(created.clone()))
        .await
        .unwrap();
    assert!(matches!(
        sdk.attach(
            &local(),
            attach_request(&created, project(temp.path(), "a"))
        )
        .await,
        Err(Error::Conflict(_))
    ));
    let attached = id("attached-first");
    sdk.attach(
        &local(),
        attach_request(&attached, project(temp.path(), "b")),
    )
    .await
    .unwrap();
    assert!(matches!(
        sdk.create(&local(), CreateRequest::new(attached.clone()))
            .await,
        Err(Error::Conflict(_))
    ));
    // A different attach for the same ID conflicts too.
    assert!(matches!(
        sdk.attach(
            &local(),
            attach_request(&attached, project(temp.path(), "c"))
        )
        .await,
        Err(Error::Conflict(_))
    ));
}

#[tokio::test]
async fn a_lost_attach_is_finished_by_its_retry() {
    let temp = TempDir::new().unwrap();
    let workspace = id("attach-retry");
    let path = project(temp.path(), "project");
    let (sdk, provider) = sdk(temp.path(), &[Next::Lost, Next::Real]);
    assert!(sdk
        .attach(&local(), attach_request(&workspace, path.clone()))
        .await
        .is_err());
    assert_eq!(
        sdk.record(&workspace).unwrap().unwrap().state,
        MachineState::Attaching
    );
    let attached = sdk
        .attach(&local(), attach_request(&workspace, path))
        .await
        .unwrap();
    assert_eq!(provider.calls(), 2);
    assert_eq!(
        sdk.record(&workspace).unwrap().unwrap().descriptor(),
        Some(&attached.descriptor())
    );
}

#[tokio::test]
async fn the_first_outcome_saved_wins_for_every_caller() {
    let temp = TempDir::new().unwrap();
    let workspace = id("race");
    let signal = Arc::new(Notify::new());
    let (sdk, provider) = sdk(
        temp.path(),
        &[Next::RealAfter(signal.clone()), Next::Refuse],
    );
    // The first caller reaches the provider and waits there.
    let slow = tokio::spawn({
        let sdk = sdk.clone();
        let workspace = workspace.clone();
        async move { sdk.create(&local(), CreateRequest::new(workspace)).await }
    });
    until(|| provider.calls() == 1).await;
    // The same request from a second caller is refused and saves that first.
    assert!(matches!(
        sdk.create(&local(), CreateRequest::new(workspace.clone()))
            .await,
        Err(Error::Refused(_))
    ));
    // The first caller's provider succeeds, but the saved refusal stands.
    signal.notify_one();
    assert!(matches!(slow.await.unwrap(), Err(Error::Refused(_))));
    assert!(matches!(
        sdk.record(&workspace).unwrap().unwrap().state,
        MachineState::Refused { .. }
    ));
}

#[tokio::test]
async fn a_forgotten_request_cannot_land_on_the_next_record() {
    let temp = TempDir::new().unwrap();
    let workspace = id("forgotten-in-flight");
    let signal = Arc::new(Notify::new());
    let (sdk, provider) = sdk(temp.path(), &[Next::RealAfter(signal.clone()), Next::Real]);
    let slow = tokio::spawn({
        let sdk = sdk.clone();
        let workspace = workspace.clone();
        async move { sdk.create(&local(), CreateRequest::new(workspace)).await }
    });
    until(|| provider.calls() == 1).await;
    let other = other_process(temp.path());
    let revision = other.record(&workspace).unwrap().unwrap().revision;
    other.forget_unfinished(&workspace, revision).unwrap();
    let attached = other
        .attach(
            &local(),
            attach_request(&workspace, project(temp.path(), "project")),
        )
        .await
        .unwrap();
    signal.notify_one();
    // The late create succeeds at its provider, but is told its record was
    // forgotten; it never adopts the attach as its own outcome. Its provider
    // work is left behind: forgetting while a call runs is what
    // `forget_unfinished` warns against.
    assert!(matches!(
        slow.await.unwrap(),
        Err(Error::Conflict(message)) if message.contains("forgotten")
    ));
    assert_eq!(
        sdk.record(&workspace).unwrap().unwrap().descriptor(),
        Some(&attached.descriptor())
    );
}

/// Another process: the same state file, its own targets, so its work and a
/// late call from the first process never touch the same directories.
fn other_process(root: &Path) -> WorkspaceSdk {
    let mut config = LocalConfig::trusted_host(root.join("other-targets"));
    config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
    WorkspaceSdk::builder(Arc::new(
        FileStateRepository::new(root.join("state")).unwrap(),
    ))
    .provider(Arc::new(LocalProvider::new(config).unwrap()))
    .unwrap()
    .build()
    .unwrap()
}

#[tokio::test]
async fn the_same_request_made_again_after_a_forget_is_not_the_late_calls_outcome() {
    let temp = TempDir::new().unwrap();
    let workspace = id("same-again");
    let signal = Arc::new(Notify::new());
    let (sdk, provider) = sdk(temp.path(), &[Next::RealAfter(signal.clone())]);
    let slow = tokio::spawn({
        let sdk = sdk.clone();
        let workspace = workspace.clone();
        async move { sdk.create(&local(), CreateRequest::new(workspace)).await }
    });
    until(|| provider.calls() == 1).await;
    let other = other_process(temp.path());
    let revision = other.record(&workspace).unwrap().unwrap().revision;
    other.forget_unfinished(&workspace, revision).unwrap();
    let again = other
        .create(&local(), CreateRequest::new(workspace.clone()))
        .await
        .unwrap();
    signal.notify_one();
    // Same request, later generation: the late call made its own target under
    // the forgotten key and must not report the new one as its own.
    assert!(matches!(
        slow.await.unwrap(),
        Err(Error::Conflict(message)) if message.contains("forgotten")
    ));
    assert_eq!(
        sdk.record(&workspace).unwrap().unwrap().descriptor(),
        Some(&again.descriptor())
    );
}

#[tokio::test]
async fn a_create_after_a_forget_never_reuses_the_forgotten_key() {
    let temp = TempDir::new().unwrap();
    let workspace = id("new-key");
    let (sdk, provider) = sdk(temp.path(), &[Next::Lost, Next::Lost, Next::Real]);
    assert!(sdk
        .create(&local(), CreateRequest::new(workspace.clone()))
        .await
        .is_err());
    let first = sdk.record(&workspace).unwrap().unwrap();
    sdk.forget_unfinished(&workspace, first.revision).unwrap();
    assert!(sdk
        .create(&local(), CreateRequest::new(workspace.clone()))
        .await
        .is_err());
    let second = sdk.record(&workspace).unwrap().unwrap();
    let MachineState::Provisioning { key } = &second.state else {
        panic!("expected a pending create");
    };
    assert_eq!(
        key,
        &ProvisionKey::for_generation(&workspace, first.revision)
    );
    // Its retry keeps the record's key, and the provider got only saved keys.
    sdk.create(&local(), CreateRequest::new(workspace.clone()))
        .await
        .unwrap();
    assert_eq!(
        provider.keys.lock().unwrap().as_slice(),
        [
            ProvisionKey::for_workspace(&workspace),
            key.clone(),
            key.clone()
        ]
    );
}

#[tokio::test]
async fn a_reconnected_target_is_never_treated_as_created_here() {
    let temp = TempDir::new().unwrap();
    let workspace = id("elsewhere");
    let (sdk, _) = sdk(temp.path(), &[]);
    let created = sdk
        .create(&local(), CreateRequest::new(workspace.clone()))
        .await
        .unwrap();
    // A second store reaches the same target only by its reference.
    let mut config = LocalConfig::trusted_host(temp.path().join("targets"));
    config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
    let other = WorkspaceSdk::builder(Arc::new(
        FileStateRepository::new(temp.path().join("other-state")).unwrap(),
    ))
    .provider(Arc::new(LocalProvider::new(config).unwrap()))
    .unwrap()
    .build()
    .unwrap();
    other.connect(&created.reference()).await.unwrap();
    assert_eq!(
        other.record(&workspace).unwrap().unwrap().origin,
        RequestOrigin::Connected
    );
    assert!(matches!(
        other.connect_owned(&created.reference()).await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        other.owned_workspace(&workspace).await,
        Err(Error::Conflict(_))
    ));
}

#[tokio::test]
async fn a_saved_target_is_never_rebound() {
    let temp = TempDir::new().unwrap();
    let workspace = id("rebind");
    let (sdk, _) = sdk(temp.path(), &[]);
    let created = sdk
        .create(&local(), CreateRequest::new(workspace.clone()))
        .await
        .unwrap();
    let repository = FileStateRepository::new(temp.path().join("state")).unwrap();
    let mut moved = created.descriptor();
    moved.reference.locator =
        WorkspaceLocator::from_locator(&serde_json::json!({"elsewhere": true})).unwrap();
    assert!(matches!(repository.save(&moved), Err(Error::Conflict(_))));
    let mut attached = created.descriptor();
    attached.ownership = Ownership::Attached;
    assert!(matches!(
        repository.save(&attached),
        Err(Error::Conflict(_))
    ));
    assert_eq!(
        repository.get(&workspace).unwrap(),
        Some(created.descriptor())
    );
}

#[tokio::test]
async fn an_upgraded_reservation_is_finished_by_the_next_create() {
    let temp = TempDir::new().unwrap();
    let workspace = id("from-v1");
    let state = temp.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    // A create that an older version reserved and never finished.
    std::fs::write(
        state.join("state.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "reservations": {"from-v1": "an-older-request-hash"},
            "workspaces": {},
            "workflows": {},
        }))
        .unwrap(),
    )
    .unwrap();
    let (sdk, provider) = sdk(temp.path(), &[]);
    assert_eq!(
        sdk.record(&workspace).unwrap().unwrap().origin,
        RequestOrigin::Upgraded
    );
    sdk.create(&local(), CreateRequest::new(workspace.clone()))
        .await
        .unwrap();
    // It asks the provider with the key the older version used.
    assert_eq!(
        provider.keys.lock().unwrap().as_slice(),
        [ProvisionKey::for_workspace(&workspace)]
    );
    let record = sdk.record(&workspace).unwrap().unwrap();
    assert!(matches!(record.origin, RequestOrigin::Request { .. }));
    assert!(matches!(record.state, MachineState::Ready { .. }));
}

fn admission(version: &str) -> Admission {
    Admission {
        environment_id: "env".into(),
        version_id: format!("id-{version}"),
        version: version.into(),
        name: "default".into(),
    }
}

#[tokio::test]
async fn a_ready_target_is_refused_only_under_its_own_admission() {
    let temp = TempDir::new().unwrap();
    let (sdk, _) = sdk(temp.path(), &[]);
    let created = sdk
        .create(&local(), CreateRequest::new(id("admitted")))
        .await
        .unwrap();
    let mut descriptor = created.descriptor();
    descriptor.admission = Some(admission("1"));
    let ready = MachineState::Ready { descriptor };
    let refused = |admitted: Option<Admission>| MachineState::Refused {
        refusal: Refusal {
            admission: admitted,
            ..refusal()
        },
    };
    assert!(ready.allows(&refused(Some(admission("1")))));
    assert!(!ready.allows(&refused(Some(admission("2")))));
    assert!(!ready.allows(&refused(None)));
}

#[test]
fn records_only_move_forward() {
    let pending = MachineState::Provisioning {
        key: ProvisionKey::for_workspace(&id("one")),
    };
    let refused = MachineState::Refused { refusal: refusal() };
    assert!(pending.allows(&refused));
    assert!(MachineState::Attaching.allows(&refused));
    assert!(!refused.allows(&pending));
    assert!(!refused.allows(&MachineState::Attaching));
    assert!(!pending.allows(&MachineState::Attaching));
    assert!(!MachineState::Attaching.allows(&pending));
}

#[tokio::test]
async fn an_upgraded_ready_target_is_kept_by_a_fitting_request_only() {
    let temp = TempDir::new().unwrap();
    let workspace = id("ready-v1");
    let created = {
        let (sdk, _) = sdk(temp.path(), &[]);
        sdk.create(&local(), CreateRequest::new(workspace.clone()))
            .await
            .unwrap()
            .descriptor()
    };
    // Rewrite the saved state as an older version would have kept it.
    std::fs::write(
        temp.path().join("state").join("state.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "reservations": {"ready-v1": "an-older-request-hash"},
            "workspaces": {"ready-v1": created},
            "workflows": {},
        }))
        .unwrap(),
    )
    .unwrap();
    let (sdk, provider) = sdk(temp.path(), &[]);
    // An attach does not fit a target the SDK created.
    assert!(matches!(
        sdk.attach(
            &local(),
            attach_request(&workspace, project(temp.path(), "p"))
        )
        .await,
        Err(Error::Conflict(_))
    ));
    // A create adopts it and reconnects without asking the provider.
    let again = sdk
        .create(&local(), CreateRequest::new(workspace.clone()))
        .await
        .unwrap();
    assert_eq!(again.descriptor(), created);
    assert_eq!(provider.calls(), 0);
}
