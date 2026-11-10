//! Commands and resource requirements for creating or attaching a workspace.
use super::{ProviderOptions, ProvisionKey, WorkspaceId};
use crate::Result;
use serde::{Deserialize, Serialize};

/// Request to create a new SDK-owned target.
///
/// The SDK may provision the target, initialises it once, and may later stop or
/// destroy it. Keep `workspace_id` stable when retrying. Configuration must match
/// the chosen provider's create options and must not contain credentials,
/// because the request can be serialized and retained between attempts.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateRequest {
    /// Caller-selected SDK identity, distinct from a cloud resource ID.
    pub workspace_id: WorkspaceId,
    /// Serialized create options for the selected provider, such as
    /// `SandboxCreateOptions`. Providers without create options require it empty.
    #[serde(default)]
    pub config: ProviderOptions,
    /// Requested target resources; unsupported requirements may be rejected.
    #[serde(default)]
    pub resources: ResourceRequirements,
}

impl CreateRequest {
    /// Request a target with the provider's default create options.
    pub fn new(workspace_id: WorkspaceId) -> Self {
        Self {
            workspace_id,
            config: ProviderOptions::default(),
            resources: ResourceRequirements::default(),
        }
    }
    /// The idempotency key every attempt to create this workspace uses.
    pub fn provision_key(&self) -> ProvisionKey {
        ProvisionKey::for_workspace(&self.workspace_id)
    }
    /// Request a target using the selected provider's named create options.
    pub fn with_options<T: Serialize>(workspace_id: WorkspaceId, options: &T) -> Result<Self> {
        Ok(Self {
            workspace_id,
            config: ProviderOptions::from_options(options)?,
            resources: ResourceRequirements::default(),
        })
    }
}

/// Request to use an existing target that someone else owns.
///
/// The SDK never provisions, stops or destroys an attached target, and never
/// changes its project files during attachment. It only prepares its own private
/// control state. Configuration names the existing target, such as
/// `LocalAttachOptions`, and must not contain credentials.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttachRequest {
    /// Caller-selected SDK identity, distinct from the target's own ID.
    pub workspace_id: WorkspaceId,
    /// Serialized attach options for the selected provider.
    pub config: ProviderOptions,
}

impl AttachRequest {
    /// Build a request from the selected provider's named attach options.
    pub fn new<T: Serialize>(workspace_id: WorkspaceId, options: &T) -> Result<Self> {
        Ok(Self {
            workspace_id,
            config: ProviderOptions::from_options(options)?,
        })
    }
}

/// Target resource requirements supplied when opening a workspace.
///
/// `None` leaves a resource unspecified. These are provider-facing allocation
/// requirements, distinct from runtime preparation’s checks of available resources.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ResourceRequirements {
    /// Requested CPU count.
    pub cpu: Option<u32>,
    /// Requested memory in mebibytes (1 MiB = 1,048,576 bytes).
    pub memory_mib: Option<u64>,
    /// Requested disk capacity in mebibytes.
    pub disk_mib: Option<u64>,
    /// Provider-specific GPU requirement or model selector.
    pub gpu: Option<String>,
    /// Provider-specific operating system or architecture selector.
    pub platform: Option<String>,
}
