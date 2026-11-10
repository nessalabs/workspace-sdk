use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
/// Options serialized into [`crate::CreateRequest`] `config` for a new Sandbox.
///
/// The root is chosen by the application; API credentials belong in the
/// injected Api client.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxCreateOptions {
    /// Absolute, normalized POSIX task namespace inside the Sandbox. Required.
    pub root: String,
    /// Provider-defined machine type at creation; None uses the provider's default.
    #[serde(default)]
    pub sandbox_type: Option<String>,
    /// Creation-time auto-stop interval in seconds. None explicitly disables auto-stop.
    #[serde(default = "default_ttl")]
    pub ttl_seconds: Option<u32>,
    /// Explicit provider environment at creation. None disables implicit environment injection.
    /// A provider with [Boat machines](super::SandboxProvider::with_machines)
    /// requires one and admits the machine only under it.
    #[serde(default)]
    pub environment: Option<String>,
    /// The snapshot a machine is made from, a snapshot id `bsh_…` or a
    /// named snapshot. Only a provider with
    /// [Boat machines](super::SandboxProvider::with_machines) takes one, and
    /// it requires one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Variables set in a machine when it is made; only a provider with
    /// [Boat machines](super::SandboxProvider::with_machines) takes them.
    /// They are part of the request and are saved and compared with it, so
    /// none may be a secret.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, String>,
}
fn default_ttl() -> Option<u32> {
    Some(3600)
}
impl SandboxCreateOptions {
    /// Select the task namespace with a one-hour auto-stop interval and no implicit environment.
    pub fn new(root: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            sandbox_type: None,
            ttl_seconds: default_ttl(),
            environment: None,
            image: None,
            variables: BTreeMap::new(),
        }
    }
}

/// Options serialized into [`crate::AttachRequest`] `config` for an existing
/// Sandbox. Attachments cannot be resumed, stopped, or deleted by this SDK.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxAttachOptions {
    /// Absolute, normalized POSIX task namespace inside the Sandbox.
    pub root: String,
    /// Existing Sandbox identifier.
    pub sandbox_id: String,
}
