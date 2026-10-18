use super::{
    client::{self, remaining},
    provider::Locator,
    SpaceConfig,
};
use crate::{
    providers::managed::{self, ITarget},
    supervisor::protocol::{
        SupervisorContext, SupervisorOperation, SupervisorRequest, SupervisorValue,
    },
    Error, Ownership, Result, WorkspaceRef, WorkspaceState,
};
use async_trait::async_trait;
use std::time::Instant;

pub(super) struct SpaceTarget {
    pub config: SpaceConfig,
    pub reference: WorkspaceRef,
    pub locator: Locator,
}
impl SpaceTarget {
    async fn inline(
        &self,
        request: &SupervisorRequest,
        deadline: Instant,
    ) -> Result<SupervisorValue> {
        let payload = serde_json::to_string(request)?;
        // Only explicitly classified non-payload operations cross argv. Shell
        // source is constant and all variable data occupies quoted positional args.
        let command = vec![
            "/bin/sh".into(),
            "-c".into(),
            "printf '%s' \"$1\" | \"$2\"".into(),
            "workspace-sdk-space".into(),
            payload,
            self.config.supervisor.clone(),
        ];
        self.invoke_command(command, deadline).await
    }
    async fn invoke_command(
        &self,
        command: Vec<String>,
        deadline: Instant,
    ) -> Result<SupervisorValue> {
        // A fixed existing cwd permits Initialize to create an absent project.
        // Workload cwd belongs to the native request, not this transport shell.
        let output = self
            .config
            .rpc(&self.locator.sandbox_id, "/", command, deadline)
            .await?;
        managed::response(&output, true)
    }
    fn scoped(&self, operation: SupervisorOperation) -> SupervisorRequest {
        SupervisorRequest::new(
            SupervisorContext {
                workspace_id: self.reference.id.clone(),
                root: self.locator.root.clone(),
                control: self.locator.control.clone(),
                ownership: Ownership::Attached,
            },
            operation,
        )
    }
}
#[async_trait]
impl ITarget for SpaceTarget {
    async fn invoke(&self, request: SupervisorRequest) -> Result<SupervisorValue> {
        let deadline = Instant::now() + self.config.timeout;
        if !request.operation.requires_private_transport() {
            return self.inline(&request, deadline).await;
        }
        let bytes = serde_json::to_vec(&request)?;
        if bytes.len() > client::MAX_REQUEST {
            return Err(Error::InvalidInput(
                "SPACE private request exceeds 16MiB".into(),
            ));
        }
        let id = uuid::Uuid::new_v4();
        let path = format!("{}/requests/{id}.json", self.locator.control);
        // Ambiguous preparation is retained. It may still be creating its slot.
        self.inline(
            &self.scoped(SupervisorOperation::PrepareRequest { id }),
            deadline,
        )
        .await?
        .empty()?;
        // A failed/ambiguous upload is deliberately NOT cleaned up: files:write
        // uses create(true), so a late writer could recreate a removed0600 slot
        // with unsafe defaults. Never invoke a possibly partial request or retry
        // its append. Retain private storage until transport quiescence is known.
        let written = self
            .config
            .upload(&self.locator.sandbox_id, &path, &bytes, deadline)
            .await?;
        let result = if written != bytes.len() as u64 {
            Err(client::failure(
                "SPACE confirmed a short private request upload",
            ))
        } else {
            self.invoke_command(
                vec![
                    self.config.supervisor.clone(),
                    "--request-file".into(),
                    path,
                ],
                deadline,
            )
            .await
        };
        // Here the upload response confirmed completion. Cleanup affects only
        // this prepared UUID, shares the remaining deadline, and cannot replace
        // the original result. Future cancellation never spawns detached cleanup.
        if remaining(deadline).is_ok() {
            let _ = self
                .inline(
                    &self.scoped(SupervisorOperation::RemoveRequest { id }),
                    deadline,
                )
                .await;
        }
        result
    }
    async fn ensure(&self) -> Result<()> {
        if self.state().await? != WorkspaceState::Ready {
            return Err(Error::State(
                "SPACE attachment is not running; its owner must manage lifecycle".into(),
            ));
        }
        Ok(())
    }
    async fn state(&self) -> Result<WorkspaceState> {
        self.config
            .sandbox_state(
                &self.locator.sandbox_id,
                Instant::now() + self.config.timeout,
            )
            .await
    }
}
