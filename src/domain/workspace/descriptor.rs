//! Workspace lifecycle observations and the capabilities of a connected target.
use super::WorkspaceRef;
use serde::{Deserialize, Serialize};

/// The identity, locations, and supported operations of an opened workspace.
///
/// Providers return this description when opening or reconnecting. The project
/// `root` contains workload files; `control_root` keeps supervisor and authentication
/// state outside synchronization. Persist [`Self::reference`] for reconnection;
/// the remaining fields describe what this connection currently reports.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BackendDescriptor {
    /// Portable handle used to reconnect through the registered provider.
    pub reference: WorkspaceRef,
    /// Absolute project directory on the target machine.
    pub root: String,
    /// Absolute private SDK directory outside the synchronized project root.
    pub control_root: String,
    /// Execution boundary supplied by this provider.
    pub isolation: Isolation,
    /// Whether the SDK created the resource or merely attached to it.
    pub ownership: Ownership,
    /// Operations the connected backend advertises.
    pub capabilities: Capabilities,
}

/// Observed lifecycle state of the workspace resource.
///
/// This is separate from [`crate::RunState`]: a ready workspace can contain no running
/// processes, and stopping a workspace is not the same as deleting its storage.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceState {
    /// The provider is creating or preparing the resource.
    Provisioning,
    /// The workspace is available for backend operations.
    Ready,
    /// The resource is stopped and may need to be resumed.
    Stopped,
    /// The workspace resource is reported deleted.
    Deleted,
    /// The backend cannot establish a recognized lifecycle state.
    Unknown,
}

/// Operations a backend supports for this workspace.
///
/// Inspect these flags before offering actions in a client UI. Support varies by
/// provider and ownership; a flag is not a guarantee that an individual call succeeds.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Can launch, inspect, reconnect to, and cancel managed runs.
    pub managed_runs: bool,
    /// Can read and write workspace files in chunks.
    pub file_transfer: bool,
    /// Can request that workspace execution stop.
    pub stop: bool,
    /// Can request deletion of SDK-owned resources.
    pub destroy: bool,
    /// Can expose an endpoint without public hosting.
    pub private_ports: bool,
    /// Can supply an authenticated hosted endpoint.
    pub hosted_ports: bool,
}

/// The execution boundary provided by a backend.
///
/// This describes the actual target rather than requesting that a trusted host
/// become a sandbox.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Isolation {
    /// Processes run directly on a host the caller explicitly trusts.
    TrustedHost,
    /// Processes run inside a container managed by the configured engine.
    Container,
    /// The cloud provider supplies the sandbox boundary.
    ProviderSandbox,
}

/// The SDK’s lifecycle relationship to a target resource.
///
/// Attaching grants access to an existing resource, not permission to destroy it.
/// Backends must enforce this distinction when handling lifecycle requests.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ownership {
    /// An existing caller-managed resource that the SDK did not create.
    Attached,
    /// A resource created and tracked by the SDK for this workspace.
    SdkOwned,
}
