use super::{configuration::SpaceOpenOptions, target::SpaceTarget, SpaceConfig};
use crate::{
    domain::*,
    ports::{IBackend, IProvider},
    providers::managed::{self, ManagedBackend},
    Error, Result,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Attach-only SPACE registration. Credentials remain in the explicit config;
/// locators bind endpoint, sandbox, user, executable and exact SDK namespace.
#[derive(Clone, Debug)]
pub struct SpaceProvider {
    id: ProviderId,
    config: SpaceConfig,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Locator {
    pub version: u32,
    pub api: String,
    pub data: String,
    pub sandbox_id: String,
    pub user: String,
    pub supervisor: String,
    pub root: String,
    pub control: String,
}
impl SpaceProvider {
    pub fn new(config: SpaceConfig) -> Self {
        Self {
            id: ProviderId::new("space").expect("static provider identity"),
            config,
        }
    }
    pub fn with_id(mut self, id: ProviderId) -> Self {
        self.id = id;
        self
    }
    fn locator(&self, id: &WorkspaceId, options: SpaceOpenOptions) -> Result<Locator> {
        options.validate()?;
        let parent = options
            .root
            .rsplit_once('/')
            .expect("validated absolute root")
            .0;
        let control = format!("{parent}/.workspace-sdk/{id}");
        super::configuration::path(&control)?;
        Ok(Locator {
            version: 1,
            api: self.config.api.as_str().into(),
            data: self.config.data.as_str().into(),
            sandbox_id: options.sandbox_id,
            user: self.config.user.clone(),
            supervisor: self.config.supervisor.clone(),
            root: options.root,
            control,
        })
    }
    fn backend(&self, reference: WorkspaceRef, locator: Locator) -> ManagedBackend {
        ManagedBackend {
            descriptor: BackendDescriptor {
                reference: reference.clone(),
                root: locator.root.clone(),
                control_root: locator.control.clone(),
                isolation: Isolation::ProviderSandbox,
                ownership: Ownership::Attached,
                capabilities: Capabilities {
                    managed_runs: true,
                    file_transfer: true,
                    ..Capabilities::default()
                },
            },
            target: Arc::new(SpaceTarget {
                config: self.config.clone(),
                reference,
                locator,
            }),
        }
    }
}
#[async_trait]
impl IProvider for SpaceProvider {
    fn id(&self) -> ProviderId {
        self.id.clone()
    }
    async fn open(&self, request: OpenRequest) -> Result<Arc<dyn IBackend>> {
        managed::require_no_resources(&request.resources)?;
        let options: SpaceOpenOptions = request.config.decode()?;
        let locator = self.locator(&request.workspace_id, options)?;
        let reference = WorkspaceRef {
            version: 1,
            id: request.workspace_id,
            provider: self.id(),
            locator: WorkspaceLocator::from_locator(&locator)?,
        };
        let backend = self.backend(reference, locator);
        backend.target.ensure().await?;
        backend.initialize(true).await?;
        backend.ensure_ready().await?;
        Ok(Arc::new(backend))
    }
    async fn connect(&self, reference: &WorkspaceRef) -> Result<Arc<dyn IBackend>> {
        if reference.version != 1 || reference.provider != self.id {
            return Err(Error::InvalidInput(
                "SPACE reference version/provider mismatch".into(),
            ));
        }
        let locator: Locator = reference.locator.decode()?;
        let expected = self.locator(
            &reference.id,
            SpaceOpenOptions::new(&locator.sandbox_id, &locator.root),
        )?;
        if locator != expected {
            return Err(Error::InvalidInput(
                "SPACE locator does not match the configured identity".into(),
            ));
        }
        let backend = self.backend(reference.clone(), locator);
        // Probe only existing native identity. Missing state is never initialized
        // or replaced during reconnection.
        backend.ensure_ready().await?;
        Ok(Arc::new(backend))
    }
}
