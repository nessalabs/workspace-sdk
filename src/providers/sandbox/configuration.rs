use serde::{Deserialize, Serialize};
/// Explicit per-workspace Sandbox creation or attachment options.
///
/// Serialize this named type into an OpenRequest. The root is chosen by the
/// application; API credentials belong in the injected Api client.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxOpenOptions {
    /// Absolute, normalized POSIX task namespace inside the Sandbox. Required.
    pub root: String,
    /// Existing Sandbox to attach to. Attachments cannot be resumed, stopped, or deleted by this SDK.
    #[serde(default)]
    pub sandbox_id: Option<String>,
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
impl SandboxOpenOptions {
    /// Select the task namespace with a one-hour auto-stop interval and no implicit environment.
    pub fn new(root: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            sandbox_id: None,
            sandbox_type: None,
            ttl_seconds: default_ttl(),
            environment: None,
        }
    }
}
