//! Commands and resource requirements for opening a workspace.
use super::{ProviderOptions, WorkspaceId};
use crate::Result;
use serde::{Deserialize, Serialize};

/// Provider-specific workspace creation or attachment request.
///
/// Keep `workspace_id` stable when retrying. Configuration must match the chosen
/// provider and must not contain credentials, because the request can be
/// serialized and retained between attempts.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpenRequest {
    /// Caller-selected SDK identity, distinct from a cloud resource ID.
    pub workspace_id: WorkspaceId,
    /// Serialized options for the selected provider, such as `SandboxOpenOptions`.
    #[serde(default)]
    pub config: ProviderOptions,
    /// Requested target resources; unsupported requirements may be rejected.
    #[serde(default)]
    pub resources: ResourceRequirements,
}

impl OpenRequest {
    /// Build a request from the selected provider’s named configuration struct.
    pub fn new<T: Serialize>(workspace_id: WorkspaceId, options: &T) -> Result<Self> {
        Ok(Self {
            workspace_id,
            config: ProviderOptions::from_options(options)?,
            resources: ResourceRequirements::default(),
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
