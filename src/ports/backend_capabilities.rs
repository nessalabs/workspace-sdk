//! Optional operations a backend may offer, one trait per capability.
//!
//! A backend that has a capability hands it out through the matching
//! [`super::IBackend`] accessor; one that lacks it returns `None` there. A
//! capability may still refuse one particular request, such as a port policy
//! the target cannot meet, and a supervisor built without `portability`
//! refuses the file-browsing and tree-transfer operations with `Unsupported`.
use super::{IConnection, SocketForward};
use crate::{domain::*, Result};
use async_trait::async_trait;
use std::sync::Arc;

/// Durable managed runs: start, observe, read output and cancel.
#[async_trait]
pub trait ManagedRuns: Send + Sync {
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
    /// Read byte-preserving retained output at independent absolute cursors.
    /// No input or process mutation occurs. Converting lossy text output back
    /// into bytes cannot satisfy this contract.
    async fn read_output_bytes(
        &self,
        run: &RunRef,
        cursor: OutputCursor,
        limit: u32,
    ) -> Result<OutputBytesPage>;
    /// Stop the identified managed process group; never signal an unrelated or guessed PID.
    async fn cancel(&self, run: &RunRef) -> Result<RunState>;
}

/// Terminal runs under the same durable run identity as managed runs.
#[async_trait]
pub trait Terminals: Send + Sync {
    /// Start a terminal. `private_cwd` selects private control state; otherwise
    /// the command's project-relative cwd applies. The supervisor owns the
    /// terminal independently of this backend handle.
    async fn start_terminal(
        &self,
        request: LaunchRequest,
        private_cwd: Option<WorkspacePath>,
        size: TerminalSize,
    ) -> Result<RunRef>;
    /// Deliver transient input once. Ambiguous acknowledgement is `Unknown`;
    /// callers must never replay the submission automatically.
    async fn write_terminal_input(&self, run: &RunRef, input: &SecretInput)
        -> Result<InputOutcome>;
    /// Resize one existing terminal without replacing its owned process.
    async fn resize_terminal(&self, run: &RunRef, size: TerminalSize) -> Result<()>;
}

/// Work in the private SDK control directory, outside the synchronized project,
/// and observation of the host that runs it. Runtime preparation needs both.
#[async_trait]
pub trait PrivateState: Send + Sync {
    /// Execute with a working directory relative to private SDK control state.
    async fn exec_private(
        &self,
        request: ExecutionRequest,
        cwd: WorkspacePath,
    ) -> Result<ExecResult>;
    /// Launch a managed run with a working directory relative to private control state.
    async fn start_private(&self, request: LaunchRequest, cwd: WorkspacePath) -> Result<RunRef>;
    /// Create a private directory beneath the control root, rejecting symlink escape.
    async fn ensure_private_directory(&self, path: WorkspacePath) -> Result<()>;
    /// Write transient credentials beneath the control root using private file permissions.
    async fn write_private_file(&self, path: WorkspacePath, bytes: &[u8]) -> Result<()>;
    /// Observe host resources without provisioning or reserving them.
    async fn inspect_resources(&self) -> Result<HostResources>;
}

/// Access to services listening in the workspace.
#[async_trait]
pub trait Ports: Send + Sync {
    /// The access policies this backend can satisfy. [`Self::port`] refuses
    /// any other policy.
    fn policies(&self) -> &[PortPolicy];
    /// Return service access satisfying the requested policy.
    async fn port(&self, request: PortRequest) -> Result<PortAccess>;
}

/// Stopping SDK-owned workspace execution. Never offered for an attached workspace.
#[async_trait]
pub trait Lifecycle: Send + Sync {
    /// Stop SDK-owned workspace execution; this does not delete anything.
    async fn stop(&self) -> Result<()>;
}

/// Deleting an SDK-owned workspace. Never offered for an attached workspace.
#[async_trait]
pub trait Deletion: Send + Sync {
    /// Request destruction and return the provider's actual cleanup progress.
    async fn destroy(&self) -> Result<DeleteOperation>;
    /// Poll a saved deletion receipt without assuming accepted means completed.
    async fn inspect_delete(&self, operation: &OperationRef) -> Result<DeleteOperation>;
}

/// Live connections into the workspace's machine.
#[async_trait]
pub trait Connections: Send + Sync {
    /// Open a new live connection into the workspace's machine, making
    /// `forward` reachable inside it when given; it ends when the last handle
    /// to it, and every stream from it, is dropped. An attached workspace
    /// may be connected too. A provider may leave something behind on the
    /// machine for each connection, such as an authorized key it cannot
    /// revoke; it says so.
    async fn connection(&self, forward: Option<SocketForward>) -> Result<Arc<dyn IConnection>>;
}

/// Revision-checked file browsing and conditional publication.
#[cfg(feature = "portability")]
#[async_trait]
pub trait FileBrowsing: Send + Sync {
    /// List one directory without recursively loading content.
    async fn list_directory(&self, request: ListDirectory) -> Result<DirectoryPage>;
    /// Read with target-side revision checks; never silently fall back to chunk reads.
    async fn read_versioned_file(&self, request: ReadVersionedFile) -> Result<VersionedFileChunk>;
    /// Publish according to an enforced precondition. Never retry an unknown outcome.
    async fn publish_file(&self, request: PublishFile) -> Result<PublishOutcome>;
}

/// Whole-tree export and import through scoped staging files.
#[cfg(feature = "portability")]
#[async_trait]
pub trait TreeTransfers: Send + Sync {
    /// Export a filtered tree to this transfer's scoped staging file.
    async fn export_tree(&self, transfer: &TreeTransfer) -> Result<TreeExportReceipt>;
    /// Restore the staged tree to an empty destination or resume an identical import.
    async fn import_tree(&self, transfer: &TreeTransfer) -> Result<TreeImportReceipt>;
    /// Remove only the temporary directory belonging to this transfer.
    async fn remove_transfer(&self, transfer: &TreeTransfer) -> Result<()>;
}
