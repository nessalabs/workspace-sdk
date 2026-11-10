//! Each workspace has one saved machine record: pending before any effect,
//! then Ready or Refused, never back. A real local provider does the work; the
//! wrapper only chooses what one create attempt reports.
use async_trait::async_trait;
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use tempfile::TempDir;
use workspace_sdk::{
    infrastructure::FileStateRepository,
    ports::IProvider,
    providers::{LocalAttachOptions, LocalConfig, LocalProvider},
    *,
};

#[derive(Clone, Copy)]
enum Next {
    /// Delegate to the real provider.
    Real,
    /// Fail with no outcome, as a lost reply or crash would.
    Lost,
    /// Refuse the target.
    Refuse,
}

struct Scripted {
    inner: LocalProvider,
    next: Mutex<Vec<Next>>,
    keys: Mutex<Vec<ProvisionKey>>,
}
#[async_trait]
impl IProvider for Scripted {
    fn id(&self) -> ProviderId {
        self.inner.id()
    }
    async fn create(&self, request: CreateRequest) -> Result<Arc<dyn IBackend>> {
        self.keys.lock().unwrap().push(request.provision_key());
        let next = self.next.lock().unwrap().pop().unwrap_or(Next::Real);
        match next {
            Next::Real => self.inner.create(request).await,
            Next::Lost => Err(Error::Provider {
                provider: "scripted".into(),
                message: "reply lost".into(),
            }),
            Next::Refuse => Err(Error::Refused(Box::new(refusal()))),
        }
    }
    async fn attach(&self, request: AttachRequest) -> Result<Arc<dyn IBackend>> {
        self.inner.attach(request).await
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
fn sdk(root: &Path, attempts: &[Next]) -> (WorkspaceSdk, Arc<Scripted>) {
    let mut config = LocalConfig::trusted_host(root.join("targets"));
    config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
    let provider = Arc::new(Scripted {
        inner: LocalProvider::new(config).unwrap(),
        next: Mutex::new(attempts.iter().rev().copied().collect()),
        keys: Mutex::new(Vec::new()),
    });
    let repository = Arc::new(FileStateRepository::new(root.join("state")).unwrap());
    let sdk = WorkspaceSdk::builder(repository)
        .provider(provider.clone())
        .unwrap()
        .build()
        .unwrap();
    (sdk, provider)
}
fn local() -> ProviderId {
    ProviderId::new("local").unwrap()
}
fn id(value: &str) -> WorkspaceId {
    WorkspaceId::new(value).unwrap()
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
    assert_eq!(provider.keys.lock().unwrap().len(), 1);
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
    assert_eq!(provider.keys.lock().unwrap().len(), 1);
    assert!(matches!(
        sdk.forget_provisioning(&workspace),
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
    sdk.forget_provisioning(&workspace).unwrap();
    assert!(sdk.record(&workspace).unwrap().is_none());
    // The ID is free again, even for an attach.
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let attached = sdk
        .attach(
            &local(),
            AttachRequest::new(workspace.clone(), &LocalAttachOptions { path: project }).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(attached.descriptor().ownership, Ownership::Attached);
    assert!(matches!(
        sdk.connect_owned(&attached.reference()).await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        sdk.forget_provisioning(&workspace),
        Err(Error::Conflict(_))
    ));
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
