use super::{Connections, Deletion, Lifecycle, ManagedRuns, Ports, PrivateState, Terminals};
#[cfg(feature = "portability")]
use super::{FileBrowsing, TreeTransfers};
use crate::{domain::*, Result};
use async_trait::async_trait;

#[async_trait]
/// Interface for a connected workspace: what every backend does, and accessors
/// for the optional capabilities.
///
/// Providers implement this contract using their native transport. Domain values
/// cross this boundary; wire-level serialization belongs inside the adapter.
/// Every backend runs foreground commands and reads and writes files. Anything
/// else is a capability: a backend that has one returns `Some(self)` from its
/// accessor, and one that lacks it keeps the default `None`. The capability
/// flags a caller sees are derived from these accessors
/// ([`dyn IBackend::capabilities`]), so they cannot disagree with them. The
/// file-browser and tree-transfer accessors exist only with the `portability`
/// feature; implementations gate them the same way.
pub trait IBackend: Send + Sync {
    /// Return connection metadata without polling remote resource state.
    /// Its `capabilities` field is replaced by [`dyn IBackend::capabilities`]
    /// wherever the SDK hands a descriptor out or saves one.
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
    /// Durable managed runs.
    fn runs(&self) -> Option<&dyn ManagedRuns> {
        None
    }
    /// Terminal runs.
    fn terminals(&self) -> Option<&dyn Terminals> {
        None
    }
    /// Private control-directory work and host inspection, for runtime preparation.
    fn private_state(&self) -> Option<&dyn PrivateState> {
        None
    }
    /// Access to listening services.
    fn ports(&self) -> Option<&dyn Ports> {
        None
    }
    /// Stopping the workspace. Never `Some` for an attached workspace.
    fn lifecycle(&self) -> Option<&dyn Lifecycle> {
        None
    }
    /// Deleting the workspace. Never `Some` for an attached workspace.
    fn deletion(&self) -> Option<&dyn Deletion> {
        None
    }
    /// Live connections into the workspace's machine.
    fn connections(&self) -> Option<&dyn Connections> {
        None
    }
    #[cfg(feature = "portability")]
    /// Revision-checked file browsing and conditional publication.
    fn file_browsing(&self) -> Option<&dyn FileBrowsing> {
        None
    }
    #[cfg(feature = "portability")]
    /// Whole-tree export and import.
    fn tree_transfers(&self) -> Option<&dyn TreeTransfers> {
        None
    }
}

impl dyn IBackend {
    /// The capability flags this backend reports, derived from its accessors.
    /// Port flags come from the policies its [`Ports`] reports.
    pub fn capabilities(&self) -> Capabilities {
        let policies = self.ports().map(Ports::policies).unwrap_or_default();
        Capabilities {
            managed_runs: self.runs().is_some(),
            file_transfer: true,
            stop: self.lifecycle().is_some(),
            destroy: self.deletion().is_some(),
            private_ports: policies.contains(&PortPolicy::Private),
            hosted_ports: policies.contains(&PortPolicy::Authenticated),
        }
    }
    /// The descriptor with its `capabilities` derived from the accessors.
    pub(crate) fn described(&self) -> BackendDescriptor {
        BackendDescriptor {
            capabilities: self.capabilities(),
            ..self.descriptor()
        }
    }
}

/// A capability the caller needs, or `Unsupported` naming it.
pub(crate) fn require<'a, T: ?Sized>(capability: Option<&'a T>, name: &str) -> Result<&'a T> {
    capability.ok_or_else(|| crate::Error::Unsupported(name.into()))
}
