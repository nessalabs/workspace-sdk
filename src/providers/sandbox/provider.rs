use super::{error::error, target::SandboxTarget, SandboxOpenOptions};
use crate::providers::managed::{self, Locator, ManagedBackend};
use crate::providers::SupervisorDeployment;
use crate::{
    domain::*,
    ports::{IBackend, IProvider},
    Error, Result,
};
use async_trait::async_trait;
use sandbox_client::{Api, CreateOptions, CreateSandboxRequest};
use sha2::{Digest, Sha256};
use std::sync::{atomic::AtomicBool, Arc};

/// Uses an injected real Api. Construct its configuration explicitly; this
/// adapter never reads API credentials from the process environment.
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
    async fn open(&self, request: OpenRequest) -> Result<Arc<dyn IBackend>> {
        if request.resources != ResourceRequirements::default() {
            return Err(Error::Unsupported(
                "Boat generic resource provisioning; choose an explicit sandbox_type in SandboxOpenOptions"
                    .into(),
            ));
        }
        let options: SandboxOpenOptions = if request.config.is_empty() {
            return Err(Error::InvalidInput(
                "SandboxOpenOptions with an explicit root is required".into(),
            ));
        } else {
            request.config.decode()?
        };
        managed::validate_remote_path(&options.root)?;
        self.supervisor.validate()?;
        let (target, ownership) = if let Some(id) = options.sandbox_id {
            self.api.get(&id).await.map_err(error)?;
            (id, Ownership::Attached)
        } else {
            let key = format!(
                "workspace-sdk-{:x}",
                Sha256::digest(request.workspace_id.as_str().as_bytes())
            );
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
                        idempotency_key: Some(key),
                        ..CreateOptions::default()
                    },
                )
                .await
                .map_err(error)?;
            (created.sandbox.id, Ownership::SdkOwned)
        };
        let directory = format!(
            "{}/{}",
            options.root.trim_end_matches('/'),
            request.workspace_id
        );
        let locator = Locator {
            root: format!("{directory}/project"),
            control: format!("{directory}/control"),
            ownership,
            target,
        };
        let reference = WorkspaceRef {
            version: 1,
            id: request.workspace_id,
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
