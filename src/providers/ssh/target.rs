use super::SshConfig;
use crate::providers::{
    managed::{self, ITarget, Locator},
    ICommandRunner, NativeCommand, SupervisorDeployment,
};
use crate::{
    domain::*,
    supervisor::protocol::{
        SupervisorContext, SupervisorOperation, SupervisorRequest, SupervisorValue,
    },
    Error, Result,
};
use async_trait::async_trait;
use std::{path::Path, sync::Arc, time::Duration};
use uuid::Uuid;

pub(super) struct SshTarget {
    pub(super) config: SshConfig,
    pub(super) runner: Arc<dyn ICommandRunner>,
    pub(super) reference: WorkspaceRef,
    pub(super) locator: Locator,
}
impl SshTarget {
    async fn native(
        &self,
        command: String,
        stdin: Vec<u8>,
        timeout: Duration,
    ) -> Result<ExecResult> {
        let args = vec![
            "-o".into(),
            "BatchMode=yes".into(),
            "-o".into(),
            "StrictHostKeyChecking=yes".into(),
            "-o".into(),
            "ConnectTimeout=10".into(),
            "-o".into(),
            "ServerAliveInterval=15".into(),
            "-o".into(),
            "ServerAliveCountMax=2".into(),
            "--".into(),
            self.config.host.clone(),
            command,
        ];
        self.runner
            .run(NativeCommand {
                program: self.config.ssh_program.clone(),
                args,
                stdin,
                timeout,
            })
            .await
    }
}
#[async_trait]
impl ITarget for SshTarget {
    async fn invoke(&self, request: SupervisorRequest) -> Result<SupervisorValue> {
        let result = self
            .native(
                managed::quote(self.config.supervisor.executable()),
                serde_json::to_vec(&request)?,
                Duration::from_secs(30),
            )
            .await?;
        managed::response(&result.stdout, result.success())
    }
    async fn ensure(&self) -> Result<()> {
        if let SupervisorDeployment::Upload {
            artifact,
            executable,
        } = &self.config.supervisor
        {
            let platform = self
                .native("uname -sm".into(), vec![], Duration::from_secs(30))
                .await?;
            if !platform.success() {
                return Err(Error::State("SSH target platform probe failed".into()));
            }
            artifact.platform.verify_uname(&platform.stdout)?;
            let bytes = artifact.bytes()?;
            let parent = Path::new(executable)
                .parent()
                .and_then(|path| path.to_str())
                .ok_or_else(|| {
                    Error::InvalidInput("supervisor executable needs an absolute parent".into())
                })?;
            let temporary = format!("{executable}.upload-{}", Uuid::new_v4());
            let command = format!(
                "umask 077; mkdir -p {} && cat > {} && chmod 700 {} && mv -f {} {}",
                managed::quote(parent),
                managed::quote(&temporary),
                managed::quote(&temporary),
                managed::quote(&temporary),
                managed::quote(executable)
            );
            let result = self
                .native(command, bytes, Duration::from_secs(180))
                .await?;
            if !result.success() {
                return Err(Error::State("SSH supervisor artifact upload failed".into()));
            }
        }
        Ok(())
    }
    async fn stop(&self) -> Result<()> {
        self.invoke(SupervisorRequest::new(
            SupervisorContext {
                workspace_id: self.reference.id.clone(),
                root: self.locator.root.clone(),
                control: self.locator.control.clone(),
                ownership: self.locator.ownership.clone(),
            },
            SupervisorOperation::StopOwnedRuns,
        ))
        .await?
        .empty()
    }
}
