use super::{target::LocalTarget, LocalAttachOptions, LocalConfig};
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
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

pub struct LocalProvider {
    id: ProviderId,
    config: LocalConfig,
    runner: Arc<dyn ICommandRunner>,
}
impl LocalProvider {
    /// Assign a registry ID when configuring multiple targets of this provider type.
    pub fn with_id(mut self, id: ProviderId) -> Self {
        self.id = id;
        self
    }

    pub fn new(config: LocalConfig) -> Result<Self> {
        Self::with_runner(config, Arc::new(SystemCommandRunner))
    }
    pub fn with_runner(config: LocalConfig, runner: Arc<dyn ICommandRunner>) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            id: ProviderId::new("local").expect("static ID"),
            config,
            runner,
        })
    }
    fn base(&self) -> Result<PathBuf> {
        fs::create_dir_all(&self.config.base_dir)?;
        Ok(self.config.base_dir.canonicalize()?)
    }
    async fn open(
        &self,
        workspace_id: WorkspaceId,
        attach: Option<PathBuf>,
    ) -> Result<Arc<dyn IBackend>> {
        let base = self.base()?;
        let directory = base.join(workspace_id.as_str());
        if directory
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(Error::InvalidInput(
                "owned task directory must not be a symlink".into(),
            ));
        }
        let ownership = if attach.is_some() {
            Ownership::Attached
        } else {
            Ownership::SdkOwned
        };
        let root = match attach {
            Some(path) => path.canonicalize()?,
            None => directory.join("project"),
        };
        let locator = Locator {
            root: path_string(&root)?,
            control: path_string(&directory.join("control"))?,
            ownership,
            target: "localhost".into(),
        };
        let reference = WorkspaceRef {
            version: 1,
            id: workspace_id,
            provider: self.id(),
            locator: WorkspaceLocator::from_locator(&locator)?,
        };
        let backend = self.backend(reference, locator, base);
        backend
            .initialize(backend.descriptor.ownership == Ownership::SdkOwned)
            .await?;
        Ok(Arc::new(backend))
    }
    fn backend(&self, reference: WorkspaceRef, locator: Locator, base: PathBuf) -> ManagedBackend {
        let owned = locator.ownership == Ownership::SdkOwned;
        ManagedBackend {
            descriptor: BackendDescriptor {
                reference: reference.clone(),
                root: locator.root.clone(),
                control_root: locator.control.clone(),
                isolation: Isolation::TrustedHost,
                ownership: locator.ownership.clone(),
                capabilities: Capabilities {
                    managed_runs: true,
                    file_transfer: true,
                    stop: owned,
                    destroy: owned,
                    private_ports: true,
                    hosted_ports: false,
                },
                admission: None,
            },
            target: Arc::new(LocalTarget {
                supervisor: self.config.supervisor.clone(),
                runner: self.runner.clone(),
                locator,
                reference,
                base,
            }),
        }
    }
}
#[async_trait]
impl IProvider for LocalProvider {
    fn id(&self) -> ProviderId {
        self.id.clone()
    }
    fn check_create(&self, request: &CreateRequest) -> Result<()> {
        managed::require_no_resources(&request.resources)?;
        managed::require_no_create_options(&request.config, "local")
    }
    fn check_attach(&self, request: &AttachRequest) -> Result<()> {
        // Reading the path changes nothing, and a missing one can never attach.
        let options: LocalAttachOptions = request.config.decode()?;
        let not_directory =
            || Error::InvalidInput("a local attach path must be a directory".into());
        let path = options
            .path
            .canonicalize()
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => not_directory(),
                _ => error.into(),
            })?;
        if !path.is_dir() {
            return Err(not_directory());
        }
        Ok(())
    }
    async fn create(&self, request: CreateRequest) -> Result<Arc<dyn IBackend>> {
        self.check_create(&request)?;
        self.open(request.workspace_id, None).await
    }
    async fn attach(&self, request: AttachRequest) -> Result<Arc<dyn IBackend>> {
        self.check_attach(&request)?;
        let options: LocalAttachOptions = request.config.decode()?;
        self.open(request.workspace_id, Some(options.path)).await
    }

    async fn connect(&self, reference: &WorkspaceRef) -> Result<Arc<dyn IBackend>> {
        let locator = managed::decode(reference, &self.id())?;
        let base = self.base()?;
        let directory = base.join(reference.id.as_str());
        if locator.target != "localhost"
            || locator.control != path_string(&directory.join("control"))?
            || (locator.ownership == Ownership::SdkOwned
                && locator.root != path_string(&directory.join("project"))?)
        {
            return Err(Error::InvalidInput(
                "local locator is outside this provider's owned task namespace".into(),
            ));
        }
        // Reconnection only validates identity. A deleted target's operation
        // receipts remain inspectable, and missing private state is not recreated.
        Ok(Arc::new(self.backend(reference.clone(), locator, base)))
    }
}
fn path_string(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::InvalidInput("provider paths must be valid UTF-8".into()))
}
