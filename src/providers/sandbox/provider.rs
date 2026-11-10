use super::{error::error, target::SandboxTarget, SandboxAttachOptions, SandboxCreateOptions};
use crate::providers::managed::{self, Locator, ManagedBackend};
use crate::providers::SupervisorDeployment;
use crate::{
    domain::*,
    ports::{IBackend, IProvider},
    Error, Result,
};
use async_trait::async_trait;
use sandbox_client::{Api, CreateOptions, CreateSandboxRequest};
use std::sync::{atomic::AtomicBool, Arc};

/// Uses an injected real Api. Construct its configuration explicitly; this
/// adapter never reads API credentials from the process environment.
///
/// Each [`IBackend::connection`](crate::ports::IBackend::connection) to one
/// of its workspaces authorizes a new SSH key on the machine, which Boat
/// cannot revoke; the key's private half is deleted with the connection.
/// Connections need `ssh` and `ssh-keygen` on this machine, and Linux with
/// `/proc` on the workspace's.
#[derive(Clone)]
pub struct SandboxProvider {
    id: ProviderId,
    api: Api,
    supervisor: SupervisorDeployment,
}
impl SandboxProvider {
    /// Assign a registry ID when configuring multiple targets of this provider type.
    pub fn with_id(mut self, id: ProviderId) -> Self {
        self.id = id;
        self
    }

    /// Configure an explicitly installed or supplied native supervisor.
    pub fn with_supervisor(mut self, supervisor: SupervisorDeployment) -> Self {
        self.supervisor = supervisor;
        self
    }

    pub fn new(api: Api) -> Self {
        Self {
            id: ProviderId::new(super::branding::DEFAULT_PROVIDER_ID).expect("static ID"),
            api,
            supervisor: SupervisorDeployment::default(),
        }
    }
    fn create_options(&self, request: &CreateRequest) -> Result<SandboxCreateOptions> {
        if request.resources != ResourceRequirements::default() {
            return Err(Error::Unsupported(
                "Boat generic resource provisioning; choose an explicit sandbox_type in SandboxCreateOptions"
                    .into(),
            ));
        }
        if request.config.is_empty() {
            return Err(Error::InvalidInput(
                "SandboxCreateOptions with an explicit root is required".into(),
            ));
        }
        let options: SandboxCreateOptions = request.config.decode()?;
        managed::validate_remote_path(&options.root)?;
        self.supervisor.validate()?;
        Ok(options)
    }
    async fn open(
        &self,
        workspace_id: WorkspaceId,
        root: &str,
        target: String,
        ownership: Ownership,
    ) -> Result<Arc<dyn IBackend>> {
        let directory = format!("{}/{}", root.trim_end_matches('/'), workspace_id);
        let locator = Locator {
            root: format!("{directory}/project"),
            control: format!("{directory}/control"),
            ownership,
            target,
        };
        let reference = WorkspaceRef {
            version: 1,
            id: workspace_id,
            provider: self.id(),
            locator: WorkspaceLocator::from_locator(&locator)?,
        };
        let creation_receipt = locator.ownership == Ownership::SdkOwned;
        let backend = self.backend(reference, locator, creation_receipt);
        // The stable create key makes a failed bootstrap retryable. Initialization
        // is only permitted here: reconnect must not recreate vanished run claims.
        backend.target.ensure().await?;
        backend.initialize(true).await?;
        Ok(Arc::new(backend))
    }
    fn backend(
        &self,
        reference: WorkspaceRef,
        locator: Locator,
        creation_receipt: bool,
    ) -> ManagedBackend {
        let owned = locator.ownership == Ownership::SdkOwned;
        ManagedBackend {
            descriptor: BackendDescriptor {
                reference: reference.clone(),
                root: locator.root.clone(),
                control_root: locator.control.clone(),
                isolation: Isolation::ProviderSandbox,
                ownership: locator.ownership.clone(),
                capabilities: Capabilities {
                    managed_runs: true,
                    file_transfer: true,
                    stop: owned,
                    destroy: owned,
                    private_ports: false,
                    hosted_ports: true,
                },
                admission: None,
            },
            target: Arc::new(SandboxTarget {
                api: self.api.clone(),
                supervisor: self.supervisor.clone(),
                locator,
                workspace_id: reference.id,
                verified_owned: AtomicBool::new(creation_receipt),
            }),
        }
    }
}
#[async_trait]
impl IProvider for SandboxProvider {
    fn id(&self) -> ProviderId {
        self.id.clone()
    }
    fn check_create(&self, request: &CreateRequest) -> Result<()> {
        self.create_options(request).map(drop)
    }
    fn check_attach(&self, request: &AttachRequest) -> Result<()> {
        let options: SandboxAttachOptions = request.config.decode()?;
        managed::validate_remote_path(&options.root)?;
        self.supervisor.validate()
    }
    async fn create(&self, request: CreateRequest) -> Result<Arc<dyn IBackend>> {
        let key = request.provision_key();
        let options = self.create_options(&request)?;
        let no_env = options.environment.is_none().then_some(true);
        let created = self
            .api
            .create(
                Some(CreateSandboxRequest {
                    type_: options.sandbox_type,
                    ttl_seconds: Some(options.ttl_seconds),
                    environment: options.environment,
                    no_env,
                    ..CreateSandboxRequest::default()
                }),
                &CreateOptions {
                    idempotency_key: Some(key.as_str().to_owned()),
                    ..CreateOptions::default()
                },
            )
            .await
            .map_err(error)?;
        self.open(
            request.workspace_id,
            &options.root,
            created.sandbox.id,
            Ownership::SdkOwned,
        )
        .await
    }
    async fn attach(&self, request: AttachRequest) -> Result<Arc<dyn IBackend>> {
        self.check_attach(&request)?;
        let options: SandboxAttachOptions = request.config.decode()?;
        self.api.get(&options.sandbox_id).await.map_err(error)?;
        self.open(
            request.workspace_id,
            &options.root,
            options.sandbox_id,
            Ownership::Attached,
        )
        .await
    }
    async fn connect(&self, reference: &WorkspaceRef) -> Result<Arc<dyn IBackend>> {
        let locator = managed::decode(reference, &self.id())?;
        let suffix = format!("/{}/project", reference.id);
        if !locator.root.ends_with(&suffix)
            || locator.control != format!("{}/control", locator.root.trim_end_matches("/project"))
        {
            return Err(Error::InvalidInput(
                "Sandbox locator is outside the workspace namespace".into(),
            ));
        }
        Ok(Arc::new(self.backend(reference.clone(), locator, false)))
    }
}
