use crate::Error;
use crate::{domain::*, Result};
use async_trait::async_trait;

#[async_trait]
/// Interface for a connected workspace’s execution, files, and lifecycle.
///
/// Providers implement this contract using their native transport. Domain values
/// cross this boundary; wire-level serialization belongs inside the adapter.
/// Optional operations default to `Unsupported` rather than silently weakening
/// the requested behavior.
pub trait IBackend: Send + Sync {
    /// Return connection metadata without polling remote resource state.
    fn descriptor(&self) -> BackendDescriptor;
    /// Establish readiness using the provider’s supported lifecycle.
    async fn ensure_ready(&self) -> Result<()>;
    /// Observe the target resource lifecycle without creating a new workspace.
    async fn refresh(&self) -> Result<WorkspaceState>;
    /// Execute a foreground project-relative command; keep credentials transient.
    async fn exec(&self, request: ExecutionRequest) -> Result<ExecResult>;
    /// Read a bounded binary chunk with symlink-safe workspace path handling.
    async fn read_file(&self, request: ReadFile) -> Result<FileChunk>;
    /// Write a bounded binary chunk, preserving the requested executable mode.
    async fn write_file(&self, request: WriteFile) -> Result<()>;
    /// Export a filtered tree to this transfer’s scoped staging file.
    async fn export_tree(&self, _transfer: &TreeTransfer) -> Result<TreeExportReceipt> {
        Err(Error::Unsupported("file-tree export".into()))
    }
    /// Restore the staged tree to an empty destination or resume an identical import.
    async fn import_tree(&self, _transfer: &TreeTransfer) -> Result<TreeImportReceipt> {
        Err(Error::Unsupported("file-tree import".into()))
    }
    /// Remove only the temporary directory belonging to this transfer.
    async fn remove_transfer(&self, _transfer: &TreeTransfer) -> Result<()> {
        Err(Error::Unsupported("file-tree transfer cleanup".into()))
    }
    /// Execute with a working directory relative to private SDK control state.
    async fn exec_private(
        &self,
        _request: ExecutionRequest,
        _cwd: WorkspacePath,
    ) -> Result<ExecResult> {
        Err(Error::Unsupported("private runtime execution".into()))
    }
    /// Launch a managed run with a working directory relative to private control state.
    async fn start_private(&self, _request: LaunchRequest, _cwd: WorkspacePath) -> Result<RunRef> {
        Err(Error::Unsupported("private runtime launch".into()))
    }
    /// Create a private directory beneath the control root, rejecting symlink escape.
    async fn ensure_private_directory(&self, _path: WorkspacePath) -> Result<()> {
        Err(Error::Unsupported("private runtime directory".into()))
    }
    /// Write transient credentials beneath the control root using private file permissions.
    async fn write_private_file(&self, _path: WorkspacePath, _bytes: &[u8]) -> Result<()> {
        Err(Error::Unsupported("private runtime file".into()))
    }
    /// Observe host resources without provisioning or reserving them.
    async fn inspect_resources(&self) -> Result<HostResources> {
        Err(Error::Unsupported("host resource inspection".into()))
    }
    /// Start or reconnect to the exact request identified by its stable run ID.
    async fn start(&self, request: LaunchRequest) -> Result<RunRef>;
    /// Observe a workspace-qualified run, rejecting references to another target.
    async fn inspect_run(&self, run: &RunRef) -> Result<RunState>;
    /// Read retained output using absolute byte cursors and an explicit page limit.
    async fn read_output(
        &self,
        run: &RunRef,
        cursor: OutputCursor,
        limit: u32,
    ) -> Result<OutputPage>;
    /// Stop the identified managed process group; never signal an unrelated or guessed PID.
    async fn cancel(&self, run: &RunRef) -> Result<RunState>;
    /// Return service access satisfying the requested policy, or reject unsupported access.
    async fn port(&self, _request: PortRequest) -> Result<PortAccess> {
        Err(Error::Unsupported("ports".into()))
    }
    /// Stop SDK-owned workspace execution; do not treat attachments as disposable.
    async fn stop(&self) -> Result<()> {
        Err(Error::Unsupported("stop".into()))
    }
    /// Request destruction and return the provider’s actual cleanup progress.
    async fn destroy(&self) -> Result<DeleteOperation> {
        Err(Error::Unsupported("destroy".into()))
    }
    /// Poll a saved deletion receipt without assuming accepted means completed.
    async fn inspect_delete(&self, _operation: &OperationRef) -> Result<DeleteOperation> {
        Err(Error::Unsupported("delete status".into()))
    }
}
