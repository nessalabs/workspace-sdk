use super::Run;
use crate::{
    domain::{
        BackendDescriptor, CommandSpec, DeleteOperation, ExecResult, ExecutionRequest, FileChunk,
        LaunchRequest, OperationRef, PortAccess, PortRequest, ReadFile, RunId, RunRef,
        WorkspaceRef, WorkspaceState, WriteFile,
    },
    ports::IBackend,
    Error, Result,
};
use std::sync::Arc;

/// A connected, identity-bearing handle to a workspace entity.
///
/// Cloning the handle does not create another target. The provider owns the
/// resource’s changing lifecycle state; [`WorkspaceRef`] is the persistable value
/// used to reconnect, and [`BackendDescriptor`] is a snapshot of its configuration.
#[derive(Clone)]
pub struct Workspace {
    pub(crate) backend: Arc<dyn IBackend>,
}
impl Workspace {
    /// Read the backend’s descriptor without polling the remote lifecycle.
    pub fn descriptor(&self) -> BackendDescriptor {
        self.backend.descriptor()
    }
    /// Return the stable, non-secret value needed to reconnect.
    pub fn reference(&self) -> WorkspaceRef {
        self.descriptor().reference
    }
    /// Access the injected backend interface for lower-level operations.
    pub fn backend(&self) -> Arc<dyn IBackend> {
        self.backend.clone()
    }
    /// Make the target ready according to its provider’s supported lifecycle.
    pub async fn ensure_ready(&self) -> Result<()> {
        self.backend.ensure_ready().await
    }
    /// Poll the provider for the target’s current lifecycle state.
    pub async fn state(&self) -> Result<WorkspaceState> {
        self.backend.refresh().await
    }
    /// Validate and execute a foreground command relative to the project root.
    pub async fn exec(&self, command: CommandSpec) -> Result<ExecResult> {
        command.validate()?;
        self.backend.exec(command.into()).await
    }
    /// Execute with transient credentials that are not part of the serializable command.
    pub async fn exec_with_secrets(&self, request: ExecutionRequest) -> Result<ExecResult> {
        request.command.validate()?;
        self.backend.exec(request).await
    }
    /// Start a managed process with a newly generated identity and an 8 MiB output limit.
    /// Use [`Self::start_with`] when the launch identity must survive request retries.
    pub async fn start(&self, command: CommandSpec) -> Result<Run> {
        self.start_with(LaunchRequest {
            id: RunId::generate(),
            execution: command.into(),
            output_limit_bytes: 8 * 1024 * 1024,
        })
        .await
    }
    /// Start or reconnect to a managed process using the supplied stable launch identity.
    pub async fn start_with(&self, request: LaunchRequest) -> Result<Run> {
        request.execution.command.validate()?;
        if !self.descriptor().capabilities.managed_runs {
            return Err(Error::Unsupported("managed runs".into()));
        }
        let reference = self.backend.start(request).await?;
        Ok(Run {
            backend: self.backend.clone(),
            reference,
        })
    }
    /// Start or reconnect to a managed process whose stdin remains open.
    ///
    /// Launch-time ordinary/private stdin is rejected before effects. Reuse the
    /// same ID for launch retries; ordinary, terminal and piped modes cannot
    /// rebind that ID. Dropping a handle does not close input or stop the run.
    /// Optional providers may reject this capability. Use explicit input EOF
    /// and confirmed cancellation/termination to release execution ownership.
    pub async fn start_piped(&self, request: LaunchRequest) -> Result<Run> {
        request.execution.command.validate()?;
        if request.execution.command.stdin.is_some() || request.execution.private_stdin.is_some() {
            return Err(Error::InvalidInput(
                "piped launch cannot include initial stdin".into(),
            ));
        }
        if !self.descriptor().capabilities.managed_runs {
            return Err(Error::Unsupported("managed runs".into()));
        }
        let reference = self.backend.start_piped(request).await?;
        Ok(Run {
            backend: self.backend.clone(),
            reference,
        })
    }
    /// Start an explicitly piped run with no inherited child environment.
    ///
    /// The executable must be an absolute target path. Explicit command values
    /// and transient secrets are applied after clearing; no PATH or HOME is
    /// supplied automatically. This does not isolate the supervisor or transport.
    /// Clean and inherited modes cannot reuse the same launch ID. All pipe input,
    /// EOF, uncertainty and execution ownership rules of `start_piped` apply.
    pub async fn start_piped_with_clean_environment(&self, request: LaunchRequest) -> Result<Run> {
        request.execution.command.validate()?;
        if !std::path::Path::new(&request.execution.command.program).is_absolute() {
            return Err(Error::InvalidInput(
                "clean environment launch requires an absolute executable".into(),
            ));
        }
        if request.execution.command.stdin.is_some() || request.execution.private_stdin.is_some() {
            return Err(Error::InvalidInput(
                "piped launch cannot include initial stdin".into(),
            ));
        }
        if !self.descriptor().capabilities.managed_runs {
            return Err(Error::Unsupported("managed runs".into()));
        }
        let reference = self
            .backend
            .start_piped_with_clean_environment(request)
            .await?;
        Ok(Run {
            backend: self.backend.clone(),
            reference,
        })
    }
    /// Construct a handle to an existing run without starting or polling a process.
    pub fn run(&self, id: RunId) -> Run {
        Run {
            backend: self.backend.clone(),
            reference: RunRef {
                workspace: self.reference(),
                id,
            },
        }
    }
    /// Read a bounded binary chunk while enforcing the workspace path boundary.
    pub async fn read_file(&self, request: ReadFile) -> Result<FileChunk> {
        self.backend.read_file(request).await
    }
    /// Write a binary chunk; this is not an atomic checkpoint publication.
    pub async fn write_file(&self, request: WriteFile) -> Result<()> {
        self.backend.write_file(request).await
    }
    /// Request access to an already-listening service with the specified access policy.
    pub async fn port(&self, request: PortRequest) -> Result<PortAccess> {
        self.backend.port(request).await
    }
    /// Request that this workspace stop; this does not imply deletion.
    pub async fn stop(&self) -> Result<()> {
        self.backend.stop().await
    }
    /// Request deletion and return a receipt. Poll it until completion is confirmed.
    pub async fn destroy(&self) -> Result<DeleteOperation> {
        self.backend.destroy().await
    }
    /// Refresh a previously recorded deletion operation.
    pub async fn inspect_delete(&self, reference: &OperationRef) -> Result<DeleteOperation> {
        self.backend.inspect_delete(reference).await
    }
}
