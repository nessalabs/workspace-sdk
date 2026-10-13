use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

// --- Account ---

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: String,
    pub user: SandboxUser,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxUser {
    pub id: Option<String>,
    pub login: Option<String>,
    pub email: Option<String>,
    pub name: Option<String>,
    pub zero_data_retention: Option<bool>,
    pub zero_data_retention_enabled_at: Option<String>,
    pub display_name: Option<String>,
    pub account_plan: Option<String>,
    pub account_status: Option<String>,
    pub closed: Option<bool>,
    pub closed_at: Option<String>,
    pub closure_contact_email: Option<String>,
    pub connection_method: Option<String>,
    pub connection_methods: Option<Vec<String>>,
    pub purge_at: Option<String>,
    pub purged_at: Option<String>,
    pub service_account: Option<bool>,
    pub suspended: Option<bool>,
    pub suspended_at: Option<String>,
    pub suspended_reason: Option<String>,
}

mod limits;
pub use limits::*;

// --- Sandbox model ---

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SandboxState {
    Init,
    Provisioning,
    Provisioned,
    Cloning,
    Ready,
    Idle,
    Running,
    Archiving,
    Archived,
    Error,
    Cancelled,
    #[serde(other)]
    Unknown,
}

impl SandboxState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Provisioning => "provisioning",
            Self::Provisioned => "provisioned",
            Self::Cloning => "cloning",
            Self::Ready => "ready",
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Archiving => "archiving",
            Self::Archived => "archived",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }

    /// Whether the Sandbox is ready to accept work.
    pub fn is_operable(&self) -> bool {
        matches!(self, Self::Ready | Self::Idle | Self::Running)
    }

    /// Whether a readiness wait must stop because the Sandbox is unavailable.
    pub fn is_terminal_failure(&self) -> bool {
        matches!(
            self,
            Self::Archived | Self::Archiving | Self::Error | Self::Cancelled
        )
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sandbox {
    pub id: String,
    pub name: Option<String>,
    pub state: SandboxState,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub vcpu: Option<u32>,
    #[serde(rename = "memoryGB")]
    pub memory_gb: Option<u32>,
    pub billing_multiplier: Option<f64>,
    pub url: Option<String>,
    pub ip: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub archive_after: Option<String>,
    #[serde(default)]
    pub desktop_available: bool,
    pub desktop_url: Option<String>,
    #[serde(default)]
    pub snapshot_available: bool,
    pub snapshot_completed_at: Option<String>,
    pub last_snapshot_attempt_at: Option<String>,
    pub last_snapshot_status: Option<String>,
    pub subdomain: Option<String>,
    pub environment: Option<String>,
    pub environment_version: Option<u32>,
    pub setup_status: Option<String>,
    pub setup_error: Option<String>,
    pub error: Option<serde_json::Value>,
    pub ssh_endpoint: Option<String>,
    /// Boat v1 `health` field.
    pub health: Option<String>,
    /// Boat v1 `healthReason` field.
    pub health_reason: Option<String>,
    /// Boat v1 `degradedSince` field.
    pub degraded_since: Option<String>,
    /// Boat v1 `hydrated` field.
    pub hydrated: Option<bool>,
    /// Boat v1 `hydratedAt` field.
    pub hydrated_at: Option<String>,
    /// Boat v1 `machineProvider` field.
    pub machine_provider: Option<String>,
    /// Boat v1 `snapshots` field.
    pub snapshots: Option<bool>,
    /// Boat v1 `monitoring` field.
    pub monitoring: Option<bool>,
    /// Boat v1 `snapshotVerifiedAt` field.
    pub snapshot_verified_at: Option<String>,
    /// Boat v1 `team` field.
    pub team: Option<serde_json::Value>,
    /// Boat v1 `createdBy` field.
    pub created_by: Option<String>,
    /// Boat v1 `createdById` field.
    pub created_by_id: Option<String>,
    /// Boat v1 `access` field.
    pub access: Option<String>,
    /// Boat v1 `holdsCreatorLogins` field.
    pub holds_creator_logins: Option<bool>,
    /// Boat v1 `wipePendingUntilRestart` field.
    pub wipe_pending_until_restart: Option<bool>,
    /// Boat v1 `stop` field.
    pub stop: Option<serde_json::Value>,
}

impl fmt::Debug for Sandbox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sandbox")
            .field("id", &self.id)
            .field("state", &self.state)
            .field("vcpu", &self.vcpu)
            .field("memory_gb", &self.memory_gb)
            .field("desktop_available", &self.desktop_available)
            .field("snapshot_available", &self.snapshot_available)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub limit: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxListResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub sandboxes: Vec<Sandbox>,
    pub page_info: Option<PageInfo>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxInfoResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    #[serde(rename = "sandbox")]
    pub sandbox: Sandbox,
    /// Boat v1 `giftLimitNotice` field.
    pub gift_limit_notice: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSandboxResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub status: Option<String>,
    pub ttl_seconds: Option<u32>,
    #[serde(rename = "sandbox")]
    pub sandbox: Sandbox,
    /// Boat v1 `giftLimitNotice` field.
    pub gift_limit_notice: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxActionResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub id: String,
    pub status: String,
    #[serde(rename = "sandbox")]
    pub sandbox: Option<Sandbox>,
    /// Boat v1 `giftLimitNotice` field.
    pub gift_limit_notice: Option<String>,
    /// Boat v1 `conversationId` field.
    pub conversation_id: Option<String>,
}

// --- Requests ---

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSandboxRequest {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    /// `None` = omit (server default). `Some(None)` = JSON null (no auto-stop).
    /// `Some(Some(n))` = n seconds.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub ttl_seconds: Option<Option<u32>>,
    /// Often holds secrets — redacted in `Debug`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_env: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup_script: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshots: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitoring: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fail_fast: Option<bool>,
}

impl fmt::Debug for CreateSandboxRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateSandboxRequest")
            .field("type_", &self.type_)
            .field("ttl_seconds", &self.ttl_seconds)
            .field("env", &self.env.as_ref().map(redact_env_map))
            .field("environment", &self.environment)
            .field("no_env", &self.no_env)
            .field(
                "setup_script",
                &self.setup_script.as_ref().map(|_| "<redacted>"),
            )
            .field("org", &self.org)
            .field("from", &self.from)
            .finish()
    }
}

impl CreateSandboxRequest {
    pub fn ttl(seconds: u32) -> Self {
        Self {
            ttl_seconds: Some(Some(seconds)),
            ..Default::default()
        }
    }

    pub fn no_auto_stop() -> Self {
        Self {
            ttl_seconds: Some(None),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSandboxRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub ttl_seconds: Option<Option<u32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subdomain: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force: Option<bool>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeRequest {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    /// Often holds secrets — redacted in `Debug`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_env: Option<bool>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub ttl_seconds: Option<Option<u32>>,
    /// Boat v1 `failFast` field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fail_fast: Option<bool>,
}

impl fmt::Debug for ResumeRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResumeRequest")
            .field("type_", &self.type_)
            .field("env", &self.env.as_ref().map(redact_env_map))
            .field("environment", &self.environment)
            .field("no_env", &self.no_env)
            .field("ttl_seconds", &self.ttl_seconds)
            .finish()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxesQuery {
    /// Per-request wallet header, separate from the organization query.
    #[serde(skip_serializing)]
    pub x_boat_org: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandRequest {
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detached: Option<bool>,
    /// Boat v1 `stream` field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

impl fmt::Debug for CommandRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandRequest")
            .field("command", &"<redacted>")
            .field("cwd", &self.cwd)
            .field("timeout_seconds", &self.timeout_seconds)
            .field("detached", &self.detached)
            .finish()
    }
}

impl CommandRequest {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            cwd: None,
            stream: None,
            timeout_seconds: None,
            detached: None,
        }
    }

    pub fn with_timeout_seconds(mut self, seconds: u32) -> Self {
        self.timeout_seconds = Some(seconds);
        self
    }

    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: Option<bool>,
    pub stderr_truncated: Option<bool>,
    pub timed_out: bool,
    pub cwd: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    /// Boat v1 `commandId` field.
    pub command_id: Option<String>,
    /// Boat v1 `oomKilled` field.
    pub oom_killed: Option<bool>,
}

impl fmt::Debug for CommandResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandResponse")
            .field("ok", &self.ok)
            .field("type_", &self.type_)
            .field("success", &self.success)
            .field("exit_code", &self.exit_code)
            .field("signal", &self.signal)
            .field("stdout_len", &self.stdout.len())
            .field("stderr_len", &self.stderr.len())
            .field("stdout_truncated", &self.stdout_truncated)
            .field("stderr_truncated", &self.stderr_truncated)
            .field("timed_out", &self.timed_out)
            .field("cwd", &self.cwd)
            .field("started_at", &self.started_at)
            .field("finished_at", &self.finished_at)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileReadQuery {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileReadResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub path: Option<String>,
    pub content: Option<String>,
    pub encoding: Option<String>,
    pub success: Option<bool>,
    pub size: Option<u64>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl fmt::Debug for FileReadResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileReadResponse")
            .field("ok", &self.ok)
            .field("type_", &self.type_)
            .field("path", &self.path)
            .field("content_len", &self.content.as_ref().map(|c| c.len()))
            .field("encoding", &self.encoding)
            .field("extra_keys", &self.extra.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileWriteRequest {
    pub path: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
}

impl fmt::Debug for FileWriteRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileWriteRequest")
            .field("path", &self.path)
            .field("content_len", &self.content.len())
            .field("encoding", &self.encoding)
            .finish()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileWriteResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub success: Option<bool>,
    pub path: Option<String>,
    pub encoding: Option<String>,
    pub size: Option<u64>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostPortRequest {
    pub port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Boat v1 `localhost` field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub localhost: Option<bool>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostPortResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub success: Option<bool>,
    pub port: Option<u16>,
    /// May include a short-lived `_token` query param — treat as secret.
    pub url: Option<String>,
    pub is_protected: Option<bool>,
    pub access: Option<String>,
    /// Boat v1 `localhost` field.
    pub localhost: Option<bool>,
}

impl std::fmt::Debug for HostPortResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostPortResponse")
            .field("ok", &self.ok)
            .field("type_", &self.type_)
            .field("success", &self.success)
            .field("port", &self.port)
            .field("url", &self.url.as_ref().map(|_| "<redacted>"))
            .field("is_protected", &self.is_protected)
            .field("access", &self.access)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshKeyRequest {
    #[serde(rename = "key")]
    pub public_key: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshKeyResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub success: Option<bool>,
    pub machine_ip: Option<String>,
    pub ssh_user: Option<String>,
    /// Boat v1 `sshEndpoint` field.
    pub ssh_endpoint: Option<String>,
    /// Boat v1 `hostKey` field.
    pub host_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiErrorBody {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub status: Option<u16>,
    pub code: Option<String>,
    pub message: Option<String>,
    pub request_id: Option<String>,
    pub error: Option<ApiErrorDetail>,
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiErrorDetail {
    pub code: Option<String>,
    pub message: Option<String>,
    pub status: Option<u16>,
    pub details: Option<serde_json::Value>,
}

fn redact_env_map(env: &HashMap<String, String>) -> HashMap<String, &'static str> {
    env.keys().map(|k| (k.clone(), "<redacted>")).collect()
}

// Requests with optional nullable fields distinguish omission, null and value.
fn deserialize_optional_nullable<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

// Runtime models are grouped by API domain and re-exported at the crate root.
mod options;
pub use options::*;
mod commands;
mod environments;
mod prompts;
mod snapshots;
pub use commands::*;
pub use environments::*;
pub use prompts::*;
pub use snapshots::*;

mod administration;
pub use administration::*;
mod webhooks;
pub use webhooks::*;
mod repositories;
pub use repositories::*;
mod environment_items;
pub use environment_items::*;
mod named_snapshots;
pub use named_snapshots::*;
mod lifecycle;
pub use lifecycle::*;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn create_request_debug_redacts_env_and_setup() {
        let mut env = HashMap::new();
        env.insert("SECRET".into(), "super-secret-value".into());
        let req = CreateSandboxRequest {
            env: Some(env),
            setup_script: Some("export TOKEN=abc".into()),
            ..Default::default()
        };
        let s = format!("{req:?}");
        assert!(s.contains("<redacted>"));
        assert!(!s.contains("super-secret-value"));
        assert!(!s.contains("export TOKEN"));
    }

    #[test]
    fn box_deserializes_without_desktop_flags() {
        let v = json!({
            "id": "bx_23456789",
            "name": "demo",
            "state": "ready"
        });
        let b: Sandbox = serde_json::from_value(v).unwrap();
        assert!(!b.desktop_available);
        assert!(!b.snapshot_available);
    }

    #[test]
    fn command_response_debug_hides_streams() {
        let res = CommandResponse {
            ok: true,
            type_: None,
            success: true,
            exit_code: Some(0),
            signal: None,
            stdout: "secret-output".into(),
            stderr: "secret-err".into(),
            stdout_truncated: None,
            stderr_truncated: None,
            timed_out: false,
            cwd: None,
            command_id: None,
            oom_killed: None,
            started_at: None,
            finished_at: None,
        };
        let s = format!("{res:?}");
        assert!(s.contains("stdout_len"));
        assert!(!s.contains("secret-output"));
        assert!(!s.contains("secret-err"));
    }
}
