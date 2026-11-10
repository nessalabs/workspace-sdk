//! Capabilities are what a backend's accessors hand out, an attached workspace
//! has no lifecycle, and a saved descriptor's flags never decide behaviour.
use std::{fs, path::Path, sync::Arc};
use tempfile::TempDir;
use workspace_sdk::{
    infrastructure::FileStateRepository,
    providers::{LocalAttachOptions, LocalConfig, LocalProvider},
    *,
};

fn provider(base: &Path) -> LocalProvider {
    let mut config = LocalConfig::trusted_host(base.join("targets"));
    config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
    LocalProvider::new(config).unwrap()
}

fn sdk(base: &Path) -> WorkspaceSdk {
    let repository = Arc::new(FileStateRepository::new(base.join("state")).unwrap());
    WorkspaceSdk::builder(repository)
        .provider(Arc::new(provider(base)))
        .unwrap()
        .build()
        .unwrap()
}

fn local() -> ProviderId {
    ProviderId::new("local").unwrap()
}

fn attach_request(base: &TempDir, id: &WorkspaceId) -> AttachRequest {
    let project = base.path().join("project");
    fs::create_dir_all(&project).unwrap();
    AttachRequest::new(id.clone(), &LocalAttachOptions { path: project }).unwrap()
}

/// The flags the accessors stand for, written out independently of the SDK.
fn present(backend: &dyn IBackend) -> Capabilities {
    let policies = backend.ports().map(|ports| ports.policies().to_vec());
    let serves = |policy| policies.as_ref().is_some_and(|all| all.contains(&policy));
    Capabilities {
        managed_runs: backend.runs().is_some(),
        file_transfer: true,
        stop: backend.lifecycle().is_some(),
        destroy: backend.deletion().is_some(),
        private_ports: serves(PortPolicy::Private),
        hosted_ports: serves(PortPolicy::Authenticated),
    }
}

#[tokio::test]
async fn local_capabilities_are_the_accessors_present() {
    let base = tempfile::tempdir().unwrap();
    let provider = provider(base.path());
    let owned = provider
        .create(CreateRequest::new(WorkspaceId::generate()))
        .await
        .unwrap();
    let attached = provider
        .attach(attach_request(&base, &WorkspaceId::generate()))
        .await
        .unwrap();
    for backend in [&owned, &attached] {
        assert_eq!(backend.descriptor().capabilities, present(backend.as_ref()));
        assert_eq!(backend.capabilities(), present(backend.as_ref()));
        assert!(backend.runs().is_some() && backend.terminals().is_some());
        assert!(backend.private_state().is_some() && backend.connections().is_none());
        assert_eq!(
            backend.ports().unwrap().policies(),
            [PortPolicy::Private].as_slice()
        );
    }
    assert!(owned.lifecycle().is_some() && owned.deletion().is_some());
    owned.deletion().unwrap().destroy().await.unwrap();
}

#[tokio::test]
async fn an_attached_workspace_has_no_lifecycle_and_cannot_be_stopped() {
    let base = tempfile::tempdir().unwrap();
    let sdk = sdk(base.path());
    let id = WorkspaceId::new("attached").unwrap();
    let workspace = sdk
        .attach(&local(), attach_request(&base, &id))
        .await
        .unwrap();
    let backend = provider(base.path())
        .connect(&workspace.reference())
        .await
        .unwrap();
    assert!(backend.lifecycle().is_none() && backend.deletion().is_none());
    let capabilities = workspace.descriptor().capabilities;
    assert!(!capabilities.stop && !capabilities.destroy);
    // `Workspace` has no stop, destroy or public backend; the owned handle,
    // the only one with them, is refused for an attached target.
    assert!(matches!(
        sdk.owned_workspace(&id).await,
        Err(Error::Conflict(_))
    ));
    // A port the target cannot serve is refused by the capability itself.
    assert!(matches!(
        workspace
            .port(PortRequest {
                port: 8080,
                policy: PortPolicy::Authenticated,
            })
            .await,
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        workspace.connection(None).await,
        Err(Error::Unsupported(_))
    ));
}

#[tokio::test]
async fn a_saved_ready_descriptor_loads_and_its_flags_decide_nothing() {
    let base = tempfile::tempdir().unwrap();
    let id = WorkspaceId::new("saved").unwrap();
    let created = sdk(base.path())
        .create(&local(), CreateRequest::new(id.clone()))
        .await
        .unwrap();
    let derived = created.descriptor().capabilities;
    drop(created);

    // Rewrite the saved flags as an older SDK might have left them: wrong in
    // every field. The file keeps the version 4 shape it always had.
    let path = base.path().join("state/state.json");
    let mut state: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(state["version"], 4);
    let saved = &mut state["machines"]["saved"]["state"]["descriptor"]["capabilities"];
    assert_eq!(
        serde_json::from_value::<Capabilities>(saved.clone()).unwrap(),
        derived
    );
    *saved = serde_json::json!({
        "managed_runs": false,
        "file_transfer": false,
        "stop": false,
        "destroy": false,
        "private_ports": false,
        "hosted_ports": true
    });
    fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();

    let sdk = sdk(base.path());
    let record = sdk.record(&id).unwrap().unwrap();
    assert!(record.descriptor().unwrap().capabilities.hosted_ports);
    // Behaviour follows the backend, not the stale flags.
    let owned = sdk.owned_workspace(&id).await.unwrap();
    assert_eq!(owned.descriptor().capabilities, derived);
    owned
        .start(CommandSpec::new("true"))
        .await
        .unwrap()
        .wait(std::time::Duration::from_secs(15))
        .await
        .unwrap();
    owned.stop().await.unwrap();
    // Reconnecting saved the derived flags over the stale ones.
    let record = sdk.record(&id).unwrap().unwrap();
    assert_eq!(record.descriptor().unwrap().capabilities, derived);
    owned.destroy().await.unwrap();
}
