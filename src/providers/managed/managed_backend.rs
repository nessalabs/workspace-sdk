use super::{ITarget, MAX_CHUNK};
use crate::{
    domain::*,
    ports::IBackend,
    supervisor::protocol::{
        SupervisorContext, SupervisorExecution, SupervisorOperation, SupervisorRequest,
        WorkingDirectory,
    },
    Error, Result,
};
use async_trait::async_trait;
use std::{sync::Arc, time::Duration};

/// Application-facing backend implemented through the typed native supervisor protocol.
pub(in crate::providers) struct ManagedBackend {
    pub descriptor: BackendDescriptor,
    pub target: Arc<dyn ITarget>,
}
impl ManagedBackend {
    fn protocol_request(&self, operation: SupervisorOperation) -> SupervisorRequest {
        SupervisorRequest::new(
            SupervisorContext {
                workspace_id: self.descriptor.reference.id.clone(),
                root: self.descriptor.root.clone(),
                control: self.descriptor.control_root.clone(),
                ownership: self.descriptor.ownership.clone(),
            },
            operation,
        )
    }
    pub async fn initialize(&self, create_project: bool) -> Result<()> {
        self.target
            .invoke(self.protocol_request(SupervisorOperation::Initialize { create_project }))
            .await?
            .empty()
    }
    fn validate_run(&self, run: &RunRef) -> Result<()> {
        if run.workspace != self.descriptor.reference {
            return Err(Error::InvalidInput(
                "run belongs to a different workspace".into(),
            ));
        }
        Ok(())
    }
    fn owned(&self) -> Result<()> {
        if self.descriptor.ownership == Ownership::Attached {
            Err(Error::Unsupported(
                "lifecycle mutation of an attached workspace".into(),
            ))
        } else {
            Ok(())
        }
    }
    async fn launch(&self, request: LaunchRequest, directory: WorkingDirectory) -> Result<RunRef> {
        request.execution.command.validate()?;
        if !(2..=16 * 1024 * 1024).contains(&request.output_limit_bytes) {
            return Err(Error::InvalidInput(
                "run output retention must be 2 bytes..=16 MiB".into(),
            ));
        }
        for (key, value) in &request.execution.secrets.0 {
            if key.is_empty() || key.contains(['\0', '=']) || value.contains('\0') {
                return Err(Error::InvalidInput(
                    "invalid secret environment value".into(),
                ));
            }
        }
        let reference = RunRef {
            workspace: self.descriptor.reference.clone(),
            id: request.id.clone(),
        };
        let execution = SupervisorExecution {
            command: request.execution.command,
            secrets: request.execution.secrets,
            private_stdin: request.execution.private_stdin,
            directory,
        };
        self.target
            .invoke(self.protocol_request(SupervisorOperation::Start {
                id: request.id,
                execution: Box::new(execution),
                output_limit_bytes: request.output_limit_bytes,
            }))
            .await?
            .run()?;
        Ok(reference)
    }
    async fn execute(
        &self,
        mut request: ExecutionRequest,
        directory: WorkingDirectory,
    ) -> Result<ExecResult> {
        request.command.timeout_seconds.get_or_insert(60);
        let run = self
            .launch(
                LaunchRequest {
                    id: RunId::generate(),
                    execution: request,
                    output_limit_bytes: 1024 * 1024,
                },
                directory,
            )
            .await?;
        let state = loop {
            let state = self.inspect_run(&run).await?;
            if !state.is_active() {
                break state;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        };
        let (mut cursor, mut stdout, mut stderr, mut output_truncated) =
            (OutputCursor::default(), String::new(), String::new(), false);
        loop {
            let page = self
                .read_output(&run, cursor.clone(), MAX_CHUNK as u32)
                .await?;
            output_truncated |= page.gap;
            stdout.push_str(&page.stdout);
            stderr.push_str(&page.stderr);
            let unchanged = page.next.stdout == cursor.stdout && page.next.stderr == cursor.stderr;
            cursor = page.next;
            if page.complete || unchanged {
                break;
            }
        }
        match state {
            RunState::Exited { exit_code } => Ok(ExecResult {
                exit_code: Some(exit_code),
                stdout,
                stderr,
                timed_out: false,
                output_truncated,
            }),
            RunState::TimedOut => Ok(ExecResult {
                exit_code: None,
                stdout,
                stderr,
                timed_out: true,
                output_truncated,
            }),
            RunState::Canceled => Ok(ExecResult {
                exit_code: None,
                stdout,
                stderr,
                timed_out: false,
                output_truncated,
            }),
            RunState::Lost { reason } => Err(Error::State(reason)),
            _ => unreachable!(),
        }
    }
}
#[async_trait]
impl IBackend for ManagedBackend {
    fn descriptor(&self) -> BackendDescriptor {
        self.descriptor.clone()
    }
    async fn ensure_ready(&self) -> Result<()> {
        self.target.ensure().await?;
        self.target
            .invoke(self.protocol_request(SupervisorOperation::Ready))
            .await?
            .empty()
    }
    async fn refresh(&self) -> Result<WorkspaceState> {
        let state = self.target.state().await?;
        if state != WorkspaceState::Ready {
            return Ok(state);
        }
        match self
            .target
            .invoke(self.protocol_request(SupervisorOperation::Ready))
            .await
        {
            Ok(value) => {
                value.empty()?;
                Ok(WorkspaceState::Ready)
            }
            Err(Error::NotFound(_)) => Ok(WorkspaceState::Unknown),
            Err(error) => Err(error),
        }
    }
    async fn exec(&self, request: ExecutionRequest) -> Result<ExecResult> {
        let cwd = request.command.cwd.clone();
        self.execute(request, WorkingDirectory::Project(cwd)).await
    }
    async fn exec_private(
        &self,
        request: ExecutionRequest,
        cwd: WorkspacePath,
    ) -> Result<ExecResult> {
        self.execute(request, WorkingDirectory::Control(cwd)).await
    }
    async fn start(&self, request: LaunchRequest) -> Result<RunRef> {
        let cwd = request.execution.command.cwd.clone();
        self.launch(request, WorkingDirectory::Project(cwd)).await
    }
    async fn start_private(&self, request: LaunchRequest, cwd: WorkspacePath) -> Result<RunRef> {
        self.launch(request, WorkingDirectory::Control(cwd)).await
    }
    async fn read_file(&self, request: ReadFile) -> Result<FileChunk> {
        if request.limit == 0 || request.limit as usize > MAX_CHUNK {
            return Err(Error::InvalidInput(
                "file read limit must be 1..=262144 bytes".into(),
            ));
        }
        let chunk = self
            .target
            .invoke(self.protocol_request(SupervisorOperation::ReadFile {
                path: request.path,
                offset: request.offset,
                limit: request.limit,
            }))
            .await?
            .file_chunk()?;
        if chunk.bytes.len() > request.limit as usize
            || request.offset.checked_add(chunk.bytes.len() as u64) != Some(chunk.next_offset)
        {
            return Err(Error::State(
                "target returned an invalid file chunk or offset".into(),
            ));
        }
        Ok(FileChunk {
            bytes: chunk.bytes,
            next_offset: chunk.next_offset,
            eof: chunk.eof,
        })
    }
    async fn write_file(&self, request: WriteFile) -> Result<()> {
        if request.bytes.len() > MAX_CHUNK {
            return Err(Error::InvalidInput(
                "file write chunk exceeds 256 KiB".into(),
            ));
        }
        self.target
            .invoke(self.protocol_request(SupervisorOperation::WriteFile {
                path: request.path,
                offset: request.offset,
                bytes: request.bytes,
                truncate: request.truncate,
                executable: request.executable,
            }))
            .await?
            .empty()
    }
    async fn inspect_run(&self, run: &RunRef) -> Result<RunState> {
        self.validate_run(run)?;
        self.target
            .invoke(self.protocol_request(SupervisorOperation::Inspect { id: run.id.clone() }))
            .await?
            .run()
    }
    async fn read_output(
        &self,
        run: &RunRef,
        cursor: OutputCursor,
        limit: u32,
    ) -> Result<OutputPage> {
        self.validate_run(run)?;
        if !(2..=MAX_CHUNK as u32).contains(&limit) {
            return Err(Error::InvalidInput(
                "output page limit must be 2..=262144 bytes".into(),
            ));
        }
        self.target
            .invoke(self.protocol_request(SupervisorOperation::Output {
                id: run.id.clone(),
                cursor,
                limit,
            }))
            .await?
            .output()
    }
    async fn cancel(&self, run: &RunRef) -> Result<RunState> {
        self.validate_run(run)?;
        self.target
            .invoke(self.protocol_request(SupervisorOperation::Cancel { id: run.id.clone() }))
            .await?
            .run()
    }
    async fn export_tree(&self, transfer: &TreeTransfer) -> Result<TreeExportReceipt> {
        self.target
            .invoke(self.protocol_request(SupervisorOperation::ExportTree {
                transfer: transfer.clone(),
            }))
            .await?
            .tree_export()
    }
    async fn import_tree(&self, transfer: &TreeTransfer) -> Result<TreeImportReceipt> {
        self.target
            .invoke(self.protocol_request(SupervisorOperation::ImportTree {
                transfer: transfer.clone(),
            }))
            .await?
            .tree_import()
    }
    async fn remove_transfer(&self, transfer: &TreeTransfer) -> Result<()> {
        self.target
            .invoke(self.protocol_request(SupervisorOperation::RemoveTransfer {
                transfer: transfer.clone(),
            }))
            .await?
            .empty()
    }
    async fn ensure_private_directory(&self, path: WorkspacePath) -> Result<()> {
        self.target
            .invoke(self.protocol_request(SupervisorOperation::EnsurePrivateDirectory { path }))
            .await?
            .empty()
    }
    async fn write_private_file(&self, path: WorkspacePath, bytes: &[u8]) -> Result<()> {
        self.target
            .invoke(
                self.protocol_request(SupervisorOperation::WritePrivateFile {
                    path,
                    bytes: bytes.to_vec(),
                }),
            )
            .await?
            .empty()
    }
    async fn inspect_resources(&self) -> Result<HostResources> {
        self.target
            .invoke(self.protocol_request(SupervisorOperation::InspectResources))
            .await?
            .resources()
    }
    async fn port(&self, request: PortRequest) -> Result<PortAccess> {
        if request.port == 0 {
            return Err(Error::InvalidInput("port must be nonzero".into()));
        }
        self.target.port(request).await
    }
    async fn stop(&self) -> Result<()> {
        self.owned()?;
        self.target.stop().await
    }
    async fn destroy(&self) -> Result<DeleteOperation> {
        self.owned()?;
        self.target.destroy(&self.descriptor.reference).await
    }
    async fn inspect_delete(&self, operation: &OperationRef) -> Result<DeleteOperation> {
        if operation.workspace != self.descriptor.reference {
            return Err(Error::InvalidInput(
                "operation belongs to a different workspace".into(),
            ));
        }
        self.target.inspect_delete(operation).await
    }
}
