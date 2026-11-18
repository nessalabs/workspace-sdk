use super::{configuration::SpaceAttachOptions, target::SpaceTarget, SpaceConfig};
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
    fn locator(&self, id: &WorkspaceId, options: SpaceAttachOptions) -> Result<Locator> {
        options.validate()?;
        let control = managed::control_beside(&options.root, id, super::configuration::path)?;
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
                // Derived from the backend's accessors when read.
                capabilities: Capabilities::default(),
                admission: None,
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
    fn check_create(&self, _request: &CreateRequest) -> Result<()> {
        Err(Error::Unsupported(
            "SPACE resources are created outside this SDK; attach to an existing sandbox".into(),
        ))
    }
    fn check_attach(&self, request: &AttachRequest) -> Result<()> {
        self.locator(&request.workspace_id, request.config.decode()?)
            .map(drop)
    }
    async fn create(&self, request: CreateRequest) -> Result<Arc<dyn IBackend>> {
        self.check_create(&request)?;
        Err(Error::Unsupported("SPACE create".into()))
    }
    async fn attach(&self, request: AttachRequest) -> Result<Arc<dyn IBackend>> {
        let options: SpaceAttachOptions = request.config.decode()?;
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
        let locator: Locator = managed::decode_reference(reference, &self.id)?;
        let expected = self.locator(
            &reference.id,
            SpaceAttachOptions::new(&locator.sandbox_id, &locator.root),
        )?;
        managed::require_same(&locator, &expected, "SPACE")?;
        let backend = self.backend(reference.clone(), locator);
        // Probe only existing native identity. Missing state is never initialized
        // or replaced during reconnection.
        backend.ensure_ready().await?;
        Ok(Arc::new(backend))
    }
}
