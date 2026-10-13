use super::{engine::*, target::ContainerTarget, ContainerConfig, ContainerOpenOptions};
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

/// Docker/Podman-compatible CLI. New workspaces use a dedicated named volume;
/// no host source directory, daemon socket, or credentials are mounted implicitly.
pub struct ContainerProvider {
    id: ProviderId,
    config: ContainerConfig,
    runner: Arc<dyn ICommandRunner>,
}
impl ContainerProvider {
    /// Assign a registry ID when configuring multiple targets of this provider type.
    pub fn with_id(mut self, id: ProviderId) -> Self {
        self.id = id;
        self
    }

    pub fn new(config: ContainerConfig) -> Result<Self> {
        Self::with_runner(config, Arc::new(SystemCommandRunner))
    }
    pub fn with_runner(config: ContainerConfig, runner: Arc<dyn ICommandRunner>) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            id: ProviderId::new("container").expect("static ID"),
            config,
            runner,
        })
    }
    async fn command(&self, args: Vec<String>) -> Result<ExecResult> {
        engine(&self.config, self.runner.as_ref(), args).await
    }
    fn backend(&self, reference: WorkspaceRef, locator: Locator) -> ManagedBackend {
        let owned = locator.ownership == Ownership::SdkOwned;
        ManagedBackend {
            descriptor: BackendDescriptor {
                reference: reference.clone(),
                root: locator.root.clone(),
                control_root: locator.control.clone(),
                isolation: Isolation::Container,
                ownership: locator.ownership.clone(),
                capabilities: Capabilities {
                    managed_runs: true,
                    file_transfer: true,
                    stop: owned,
                    destroy: owned,
                    ..Capabilities::default()
                },
            },
            target: Arc::new(ContainerTarget {
                config: self.config.clone(),
                runner: self.runner.clone(),
                reference,
                locator,
            }),
        }
    }
}
#[async_trait]
impl IProvider for ContainerProvider {
    fn id(&self) -> ProviderId {
        self.id.clone()
    }
    async fn open(&self, request: OpenRequest) -> Result<Arc<dyn IBackend>> {
        let options: ContainerOpenOptions = if request.config.is_empty() {
            ContainerOpenOptions::default()
        } else {
            request.config.decode()?
        };
        if request.resources.disk_mib.is_some() {
            return Err(Error::Unsupported("portable container disk quotas".into()));
        }
        let owned = options.container_id.is_none();
        if !owned {
            managed::require_no_resources(&request.resources)?;
        }
        let target = options
            .container_id
            .unwrap_or_else(|| format!("workspace-sdk-{}", request.workspace_id));
        name(&target)?;
        succeeded(
            self.command(vec!["info".into(), "--format".into(), "{{json .}}".into()])
                .await?,
            "preflight",
        )?;
        let inspected = self
            .command(vec![
                "inspect".into(),
                "--format".into(),
                "{{json .}}".into(),
                target.clone(),
            ])
            .await?;
        if inspected.success() {
            let value = container_info(&inspected)?;
            if owned {
                check_owner(&value, &request.workspace_id)?;
            }
        } else if owned {
            let volume = target.clone();
            let inspected_volume = self
                .command(vec![
                    "volume".into(),
                    "inspect".into(),
                    "--format".into(),
                    "{{json .}}".into(),
                    volume.clone(),
                ])
                .await?;
            if inspected_volume.success() {
                let existing = volume_info(&inspected_volume)?;
                check_volume_owner(&existing, &request.workspace_id)?;
            } else {
                succeeded(
                    self.command(vec![
                        "volume".into(),
                        "create".into(),
                        "--label".into(),
                        format!("dev.workspace-sdk.workspace={}", request.workspace_id),
                        volume.clone(),
                    ])
                    .await?,
                    "create owned volume",
                )?;
            }
            let mut args = vec![
                "run".into(),
                "--detach".into(),
                "--name".into(),
                target.clone(),
                "--label".into(),
                format!("dev.workspace-sdk.workspace={}", request.workspace_id),
                "--mount".into(),
                format!("type=volume,source={volume},target={}", self.config.root),
                "--workdir".into(),
                self.config.root.clone(),
            ];
            if let Some(cpu) = request.resources.cpu {
                args.extend(["--cpus".into(), cpu.to_string()]);
            }
            if let Some(memory) = request.resources.memory_mib {
                args.extend(["--memory".into(), format!("{memory}m")]);
            }
            if let Some(platform) = request.resources.platform {
                args.extend(["--platform".into(), platform]);
            }
            if let Some(gpu) = request.resources.gpu {
                args.extend(["--gpus".into(), gpu]);
            }
            args.extend([
                self.config.image.clone(),
                "sh".into(),
                "-c".into(),
                "trap 'exit 0' TERM INT; while :; do sleep 3600 & wait $!; done".into(),
            ]);
            succeeded(self.command(args).await?, "create")?;
        } else {
            return Err(Error::NotFound("attached container is unavailable".into()));
        }
        let directory = format!(
            "{}/{}",
            self.config.root.trim_end_matches('/'),
            request.workspace_id
        );
        let locator = Locator {
            root: format!("{directory}/project"),
            control: format!("{directory}/control"),
            ownership: if owned {
                Ownership::SdkOwned
            } else {
                Ownership::Attached
            },
            target,
        };
        let reference = WorkspaceRef {
            version: 1,
            id: request.workspace_id,
            provider: self.id(),
            locator: WorkspaceLocator::from_locator(&locator)?,
        };
        let backend = self.backend(reference, locator);
        backend.target.ensure().await?;
        backend.initialize(true).await?;
        Ok(Arc::new(backend))
    }
    async fn connect(&self, reference: &WorkspaceRef) -> Result<Arc<dyn IBackend>> {
        let locator = managed::decode(reference, &self.id())?;
        name(&locator.target)?;
        let directory = format!(
            "{}/{}",
            self.config.root.trim_end_matches('/'),
            reference.id
        );
        if locator.root != format!("{directory}/project")
            || locator.control != format!("{directory}/control")
            || (locator.ownership == Ownership::SdkOwned
                && locator.target != format!("workspace-sdk-{}", reference.id))
        {
            return Err(Error::InvalidInput(
                "container locator does not match configured workspace namespace".into(),
            ));
        }
        Ok(Arc::new(self.backend(reference.clone(), locator)))
    }
}
