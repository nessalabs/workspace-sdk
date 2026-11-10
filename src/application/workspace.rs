use super::Run;
use crate::{
    domain::{
        BackendDescriptor, CommandSpec, DeleteOperation, ExecResult, ExecutionRequest, FileChunk,
        LaunchRequest, OperationRef, Ownership, PortAccess, PortRequest, ReadFile, RunId, RunRef,
        WorkspaceRef, WorkspaceState, WriteFile,
    },
    ports::{require, IBackend, IConnection, SocketForward},
    Error, Result,
};
use std::sync::Arc;

/// A connected, identity-bearing handle to a workspace entity.
///
/// Cloning the handle does not create another target. The provider owns the
/// resource’s changing lifecycle state; [`WorkspaceRef`] is the persistable value
/// used to reconnect, and [`BackendDescriptor`] is a snapshot of its configuration.
/// It never stops or deletes the target: only [`OwnedWorkspace`] does.
#[derive(Clone)]
pub struct Workspace {
    /// Not public: it would reach the backend's stop and destroy.
    pub(crate) backend: Arc<dyn IBackend>,
}
impl Workspace {
    /// Read the backend’s descriptor without polling the remote lifecycle.
    /// Its capabilities are derived from what the backend offers.
    pub fn descriptor(&self) -> BackendDescriptor {
        self.backend.described()
    }
    /// Return the stable, non-secret value needed to reconnect.
    pub fn reference(&self) -> WorkspaceRef {
        self.descriptor().reference
    }
    /// Runtime preparation bound to this workspace. The host gives no way to
    /// stop or delete it.
    pub fn runtime_host(&self) -> crate::runtime::BackendRuntimeHost {
        crate::runtime::BackendRuntimeHost::new(self.backend.clone())
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
        let reference = require(self.backend.runs(), "managed runs")?
            .start(request)
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
    #[cfg(feature = "portability")]
    /// Observe one expanded directory through caller-driven complete reconciliation.
    /// No watcher is installed. Poll on your timer, back off on failure, and reset
    /// the UI when the first result says reset. Drop to release retained metadata.
    pub fn observe_directory(&self, path: crate::WorkspacePath) -> super::DirectoryObserver {
        super::DirectoryObserver::new(self.clone(), path)
    }
    #[cfg(feature = "portability")]
    /// List a bounded directory page; stale continuations must restart from the beginning.
    pub async fn list_directory(
        &self,
        request: crate::ListDirectory,
    ) -> Result<crate::DirectoryPage> {
        require(self.backend.file_browsing(), "file browsing")?
            .list_directory(request)
            .await
    }
    #[cfg(feature = "portability")]
    /// Read bytes tied to a revision. Keep its token across chunks and reject stale buffers.
    pub async fn read_versioned_file(
        &self,
        request: crate::ReadVersionedFile,
    ) -> Result<crate::VersionedFileChunk> {
        require(self.backend.file_browsing(), "file browsing")?
            .read_versioned_file(request)
            .await
    }
    #[cfg(feature = "portability")]
    /// Publish only where the target enforces the requested precondition.
    pub async fn publish_file(&self, request: crate::PublishFile) -> Result<crate::PublishOutcome> {
        require(self.backend.file_browsing(), "file browsing")?
            .publish_file(request)
            .await
    }
    /// Request access to an already-listening service with the specified access policy.
    pub async fn port(&self, request: PortRequest) -> Result<PortAccess> {
        require(self.backend.ports(), "ports")?.port(request).await
    }
    /// Open a new live connection into the workspace's machine, making
    /// `forward` reachable inside it when given. It ends when the last handle
    /// to it, and every stream from it, is dropped. See
    /// [`crate::ports::Connections`].
    pub async fn connection(&self, forward: Option<SocketForward>) -> Result<Arc<dyn IConnection>> {
        require(self.backend.connections(), "live connections")?
            .connection(forward)
            .await
    }
}

/// A workspace this SDK created, so it may also stop and destroy the target.
///
/// Every workspace operation is available through `Deref`. Only
/// [`crate::WorkspaceSdk::create`], [`crate::WorkspaceSdk::connect_owned`] and
/// [`crate::WorkspaceSdk::owned_workspace`] return one; an attached target never
/// becomes an `OwnedWorkspace`.
#[derive(Clone)]
pub struct OwnedWorkspace {
    pub(crate) workspace: Workspace,
}
impl OwnedWorkspace {
    /// Request that this workspace stop; this does not imply deletion.
    pub async fn stop(&self) -> Result<()> {
        require(self.workspace.backend.lifecycle(), "stop")?
            .stop()
            .await
    }
    /// Request deletion and return a receipt. Poll it until completion is confirmed.
    pub async fn destroy(&self) -> Result<DeleteOperation> {
        require(self.workspace.backend.deletion(), "destroy")?
            .destroy()
            .await
    }
    /// Refresh a previously recorded deletion operation.
    pub async fn inspect_delete(&self, reference: &OperationRef) -> Result<DeleteOperation> {
        require(self.workspace.backend.deletion(), "delete status")?
            .inspect_delete(reference)
            .await
    }
    /// Wrap a handle whose saved record says this SDK created it.
    pub(crate) fn from_owned(workspace: Workspace) -> Result<Self> {
        if workspace.descriptor().ownership != Ownership::SdkOwned {
            return Err(Error::Conflict(
                "an attached workspace cannot be stopped or destroyed by this SDK".into(),
            ));
        }
        Ok(Self { workspace })
    }
}
impl std::ops::Deref for OwnedWorkspace {
    type Target = Workspace;
    fn deref(&self) -> &Workspace {
        &self.workspace
    }
}
impl From<OwnedWorkspace> for Workspace {
    fn from(owned: OwnedWorkspace) -> Self {
        owned.workspace
    }
}
