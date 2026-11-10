use serde::{Deserialize, Serialize};
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
    #[serde(default)]
    pub environment: Option<String>,
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
