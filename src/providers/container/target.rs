use super::{engine::*, ContainerConfig};
use crate::providers::SupervisorDeployment;
use crate::providers::{
    managed::{self, ITarget, Locator},
    ICommandRunner, NativeCommand,
};
use crate::supervisor::protocol::{SupervisorRequest, SupervisorValue};
use crate::{domain::*, Error, Result};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc, time::Duration};
use uuid::Uuid;

pub(super) struct ContainerTarget {
    pub(super) config: ContainerConfig,
    pub(super) runner: Arc<dyn ICommandRunner>,
    pub(super) reference: WorkspaceRef,
    pub(super) locator: Locator,
}
impl ContainerTarget {
    fn deletion_receipt(&self, state: DeleteState, detail: Option<&str>) -> DeleteOperation {
        let identity = format!(
            "{}\0{}\0{}",
            self.reference.provider, self.reference.id, self.locator.target
        );
        let id = OperationId::new(format!(
            "container-delete-{:x}",
            Sha256::digest(identity.as_bytes())
        ))
        .expect("safe digest identifier");
        DeleteOperation {
            reference: OperationRef {
                workspace: self.reference.clone(),
                id,
            },
            state,
            detail: detail.map(str::to_owned),
        }
    }
    async fn remaining_resources(&self) -> Result<(bool, bool)> {
        // A failed inspect does not prove absence. Successful, narrowly filtered
        // engine inventories confirm whether the exact names still exist.
        let (containers, volumes) = tokio::try_join!(
            self.command(vec![
                "container".into(),
                "ls".into(),
                "--all".into(),
                "--filter".into(),
                format!("name={}", self.locator.target),
                "--format".into(),
                "{{.Names}}".into()
            ]),
            self.command(vec![
                "volume".into(),
                "ls".into(),
                "--filter".into(),
                format!("name={}", self.locator.target),
                "--format".into(),
                "{{.Name}}".into()
            ])
        )?;
        let containers = succeeded(containers, "list target container")?;
        let volumes = succeeded(volumes, "list target volume")?;
        Ok((
            containers
                .stdout
                .lines()
                .any(|name| name.trim() == self.locator.target),
            volumes
                .stdout
                .lines()
                .any(|name| name.trim() == self.locator.target),
        ))
    }
    async fn verify_volume(&self) -> Result<()> {
        let volume = volume_info(&succeeded(
            self.command(vec![
                "volume".into(),
                "inspect".into(),
                "--format".into(),
                "{{json .}}".into(),
                self.locator.target.clone(),
            ])
            .await?,
            "inspect owned volume",
        )?)?;
        check_volume_owner(&volume, &self.reference.id)?;
        Ok(())
    }
    async fn command(&self, args: Vec<String>) -> Result<ExecResult> {
        engine(&self.config, self.runner.as_ref(), args).await
    }
    async fn inspect(&self) -> Result<ContainerDetails> {
        let result = succeeded(
            self.command(vec![
                "inspect".into(),
                "--format".into(),
                "{{json .}}".into(),
                self.locator.target.clone(),
            ])
            .await?,
            "inspect",
        )?;
        let value = container_info(&result)?;
        if self.locator.ownership == Ownership::SdkOwned {
            check_owner(&value, &self.reference.id)?;
        }
        Ok(value)
    }
}
#[async_trait]
impl ITarget for ContainerTarget {
    async fn invoke(&self, request: SupervisorRequest) -> Result<SupervisorValue> {
        let args = vec![
            "exec".into(),
            "--interactive".into(),
            self.locator.target.clone(),
            self.config.supervisor.executable().into(),
        ];
        let result = self
            .runner
            .run(NativeCommand {
                program: self.config.engine.clone(),
                args,
                stdin: serde_json::to_vec(&request)?,
                timeout: Duration::from_secs(30),
            })
            .await?;
        managed::response(&result.stdout, result.success())
    }
    async fn ensure(&self) -> Result<()> {
        let value = self.inspect().await?;
        if !value.state.running && self.locator.ownership == Ownership::Attached {
            return Err(Error::State(
                "attached container is stopped; its owner must start it".into(),
            ));
        }
        if !value.state.running {
            succeeded(
                self.command(vec!["start".into(), self.locator.target.clone()])
                    .await?,
                "start",
            )?;
        }
        if let SupervisorDeployment::Upload {
            artifact,
            executable,
        } = &self.config.supervisor
        {
            let platform = succeeded(
                self.command(vec![
                    "exec".into(),
                    self.locator.target.clone(),
                    "uname".into(),
                    "-sm".into(),
                ])
                .await?,
                "probe supervisor platform",
            )?;
            artifact.platform.verify_uname(&platform.stdout)?;
            artifact.bytes()?;
            let parent = Path::new(executable)
                .parent()
                .and_then(|path| path.to_str())
                .ok_or_else(|| {
                    Error::InvalidInput("supervisor executable needs an absolute parent".into())
                })?;
            let temporary = format!("{executable}.upload-{}", Uuid::new_v4());
            succeeded(
                self.command(vec![
                    "exec".into(),
                    self.locator.target.clone(),
                    "mkdir".into(),
                    "-p".into(),
                    parent.into(),
                ])
                .await?,
                "prepare supervisor upload",
            )?;
            let source = artifact
                .path
                .canonicalize()?
                .to_str()
                .ok_or_else(|| Error::InvalidInput("artifact path must be UTF-8".into()))?
                .to_owned();
            succeeded(
                self.command(vec![
                    "cp".into(),
                    source,
                    format!("{}:{temporary}", self.locator.target),
                ])
                .await?,
                "upload supervisor",
            )?;
            succeeded(
                self.command(vec![
                    "exec".into(),
                    self.locator.target.clone(),
                    "chmod".into(),
                    "700".into(),
                    temporary.clone(),
                ])
                .await?,
                "set supervisor permissions",
            )?;
            succeeded(
                self.command(vec![
                    "exec".into(),
                    self.locator.target.clone(),
                    "mv".into(),
                    "-f".into(),
                    temporary,
                    executable.clone(),
                ])
                .await?,
                "install supervisor",
            )?;
        }
        Ok(())
    }
    async fn state(&self) -> Result<WorkspaceState> {
        Ok(if self.inspect().await?.state.running {
            WorkspaceState::Ready
        } else {
            WorkspaceState::Stopped
        })
    }
    async fn stop(&self) -> Result<()> {
        self.inspect().await?;
        succeeded(
            self.command(vec![
                "stop".into(),
                "--time".into(),
                "10".into(),
                self.locator.target.clone(),
            ])
            .await?,
            "stop",
        )?;
        Ok(())
    }
    async fn destroy(&self, _reference: &WorkspaceRef) -> Result<DeleteOperation> {
        let (container_exists, volume_exists) = self.remaining_resources().await?;
        if container_exists {
            let container = self.inspect().await?;
            let owned_mount = container.mounts.iter().any(|mount| {
                mount.mount_type == "volume"
                    && mount.destination == self.config.root
                    && mount.name.as_deref() == Some(self.locator.target.as_str())
            });
            if !owned_mount {
                return Err(Error::Conflict(
                    "container does not use this workspace's owned volume".into(),
                ));
            }
        }
        if volume_exists {
            self.verify_volume().await?;
        }
        if container_exists {
            match self.command(vec!["rm".into(),"--force".into(),self.locator.target.clone()]).await {
                Ok(result) if result.success() => {},
                Ok(_) => return Ok(self.deletion_receipt(DeleteState::Blocked, Some("Container removal failed; no volume removal was attempted. Retry destroy with the same workspace reference."))),
                Err(_) => return Ok(self.deletion_receipt(DeleteState::Unknown, Some("Container removal outcome is unknown. Inspect this receipt before retrying."))),
            }
        }
        if volume_exists {
            match self.command(vec!["volume".into(),"rm".into(),self.locator.target.clone()]).await {
                Ok(result) if result.success() => {},
                Ok(_) => return Ok(self.deletion_receipt(DeleteState::Blocked, Some("Container is removed, but its owned volume remains. Retry destroy with the same workspace reference to remove the volume."))),
                Err(_) => return Ok(self.deletion_receipt(DeleteState::Unknown, Some("Container is removed; owned-volume removal outcome is unknown. Inspect this receipt before retrying."))),
            }
        }
        match self.remaining_resources().await {
            Ok((false,false)) => Ok(self.deletion_receipt(DeleteState::Completed,None)),
            Ok(_) => Ok(self.deletion_receipt(DeleteState::Blocked,Some("The engine still reports a target resource. Retry destroy with the same workspace reference."))),
            Err(_) => Ok(self.deletion_receipt(DeleteState::Unknown,Some("Removal commands completed, but absence could not be confirmed. Inspect this receipt again."))),
        }
    }
    async fn inspect_delete(&self, operation: &OperationRef) -> Result<DeleteOperation> {
        let expected = self.deletion_receipt(DeleteState::Unknown, None);
        if operation.id != expected.reference.id {
            return Err(Error::InvalidInput(
                "container deletion receipt ID does not match this workspace".into(),
            ));
        }
        match self.remaining_resources().await? {
            (false,false) => Ok(self.deletion_receipt(DeleteState::Completed,None)),
            (false,true) => Ok(self.deletion_receipt(DeleteState::Blocked,Some("The container is absent and its owned volume remains. Retry destroy with the same workspace reference."))),
            _ => Ok(self.deletion_receipt(DeleteState::Blocked,Some("A target resource remains. Retry destroy with the same workspace reference."))),
        }
    }
}
