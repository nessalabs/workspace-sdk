use super::{target::SshTarget, SshConfig, SshOpenOptions};
use crate::providers::{
    managed::{self, Locator, ManagedBackend},
    ICommandRunner, SystemCommandRunner,
};
use crate::{
    domain::*,
    ports::{IBackend, IProvider},
    Error, Result,
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct SshProvider {
    id: ProviderId,
    config: SshConfig,
    runner: Arc<dyn ICommandRunner>,
}
impl SshProvider {
    /// Assign a registry ID when configuring multiple targets of this provider type.
    pub fn with_id(mut self, id: ProviderId) -> Self {
        self.id = id;
        self
    }

    pub fn new(config: SshConfig) -> Result<Self> {
        Self::with_runner(config, Arc::new(SystemCommandRunner))
    }
    pub fn with_runner(config: SshConfig, runner: Arc<dyn ICommandRunner>) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            id: ProviderId::new("ssh").expect("static ID"),
            config,
            runner,
        })
    }
    fn backend(&self, reference: WorkspaceRef, locator: Locator) -> ManagedBackend {
        let owned = locator.ownership == Ownership::SdkOwned;
        ManagedBackend {
            descriptor: BackendDescriptor {
                reference: reference.clone(),
                root: locator.root.clone(),
                control_root: locator.control.clone(),
                ownership: locator.ownership.clone(),
                isolation: Isolation::TrustedHost,
                capabilities: Capabilities {
                    managed_runs: true,
                    file_transfer: true,
                    stop: owned,
                    ..Capabilities::default()
                },
            },
            target: Arc::new(SshTarget {
                config: self.config.clone(),
                runner: self.runner.clone(),
                reference,
                locator,
            }),
        }
    }
}
#[async_trait]
impl IProvider for SshProvider {
    fn id(&self) -> ProviderId {
        self.id.clone()
    }
    async fn open(&self, request: OpenRequest) -> Result<Arc<dyn IBackend>> {
        managed::require_no_resources(&request.resources)?;
        let options: SshOpenOptions = if request.config.is_empty() {
            SshOpenOptions::default()
        } else {
            request.config.decode()?
        };
        let directory = format!(
            "{}/{}",
            self.config.root.trim_end_matches('/'),
            request.workspace_id
        );
        let ownership = if options.attach.is_some() {
            Ownership::Attached
        } else {
            Ownership::SdkOwned
        };
        let locator = Locator {
            root: options
                .attach
                .unwrap_or_else(|| format!("{directory}/project")),
            control: format!("{directory}/control"),
            ownership,
            target: self.config.host.clone(),
        };
        managed::validate_remote_path(&locator.root)?;
        let reference = WorkspaceRef {
            version: 1,
            id: request.workspace_id,
            provider: self.id(),
            locator: WorkspaceLocator::from_locator(&locator)?,
        };
        let backend = self.backend(reference, locator);
        backend.target.ensure().await?;
        backend
            .initialize(backend.descriptor.ownership == Ownership::SdkOwned)
            .await?;
        Ok(Arc::new(backend))
    }
    async fn connect(&self, reference: &WorkspaceRef) -> Result<Arc<dyn IBackend>> {
        let locator = managed::decode(reference, &self.id())?;
        let directory = format!(
            "{}/{}",
            self.config.root.trim_end_matches('/'),
            reference.id
        );
        if locator.target != self.config.host
            || locator.control != format!("{directory}/control")
            || (locator.ownership == Ownership::SdkOwned
                && locator.root != format!("{directory}/project"))
        {
            return Err(Error::InvalidInput(
                "SSH locator is outside the configured target namespace".into(),
            ));
        }
        Ok(Arc::new(self.backend(reference.clone(), locator)))
    }
}
