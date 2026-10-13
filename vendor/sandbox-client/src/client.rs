use crate::service::SANDBOX_PATH;
mod administration;
mod transport;

use std::time::Duration;

use reqwest::{Client, Method};

use crate::config::ClientConfig;
use crate::error::{Error, Result};
use crate::types::*;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

// Characters allowed unescaped inside an API path segment.
const PATH_SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~')
    .remove(b'!')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')')
    .remove(b'*');

/// Boat id pattern: `bx_` + 8 chars from a Crockford-like alphabet.
const SANDBOX_ID_PREFIX: &str = "bx_";
const SANDBOX_ID_BODY_LEN: usize = 8;
const SANDBOX_ID_ALPHABET: &[u8] = b"23456789abcdefghjkmnpqrstuvwxyz";

/// Asynchronous client for the Boat API.
///
/// Cheap to clone: wraps a pooled `reqwest::Client`.
#[derive(Clone, Debug)]
pub struct Api {
    http: Client,
    config: ClientConfig,
    base: String,
}

impl Api {
    pub fn new(config: ClientConfig) -> Result<Self> {
        crate::config::validate_base_path(&config.base_path)?;
        config.retry.validate()?;
        let http = Client::builder()
            .connect_timeout(config.connect_timeout)
            .timeout(config.request_timeout)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_nodelay(true)
            .user_agent(&config.user_agent)
            .build()
            .map_err(|e| Error::Config(format!("failed to build HTTP client: {e}")))?;
        let base = config.base_path.trim_end_matches('/').to_string();
        Ok(Self { http, config, base })
    }

    /// Return an independently scoped clone. Environment operations use `?org=`;
    /// other operations use the wallet header. `None` clears explicit scope:
    /// sandbox operations use the account's active wallet, while environments
    /// use personal scope. Use `Some("personal".into())` for explicit personal billing.
    pub fn with_org(mut self, org: Option<String>) -> Self {
        self.config.org = org;
        self
    }

    /// Read the configuration used by this client.
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    // --- Account ---

    /// Read the authenticated account profile.
    pub async fn me(&self) -> Result<MeResponse> {
        self.request("me", Method::GET, "/me").send().await
    }

    /// Read limits for an explicit organization or team scope.
    pub async fn limits(&self, options: &LimitsOptions) -> Result<LimitsResponse> {
        self.request_with_org(
            "limits",
            Method::GET,
            "/limits",
            options.x_boat_org.as_deref(),
        )
        .query(Some(options))
        .send()
        .await
    }

    // --- Lifecycle ---

    /// List Sandboxes with optional state filters and pagination.
    pub async fn sandboxes(&self, query: Option<&SandboxesQuery>) -> Result<SandboxListResponse> {
        self.request_with_org(
            "sandboxes",
            Method::GET,
            SANDBOX_PATH,
            query.and_then(|q| q.x_boat_org.as_deref()),
        )
        .query(query)
        .send()
        .await
    }

    /// Create a Sandbox with optional body, organization scope, and idempotency key.
    pub async fn create(
        &self,
        request: Option<CreateSandboxRequest>,
        options: &CreateOptions,
    ) -> Result<CreateSandboxResponse> {
        self.request_with_org(
            "create",
            Method::POST,
            SANDBOX_PATH,
            options.x_boat_org.as_deref(),
        )
        .json(request.as_ref())
        .query(options.org.as_ref().map(|org| [("org", org)]).as_ref())
        .header("Idempotency-Key", options.idempotency_key.as_deref())
        .send()
        .await
    }

    /// Read the current state and configuration of a Sandbox.
    pub async fn get(&self, sandbox_id: &str) -> Result<SandboxInfoResponse> {
        self.request(
            "get",
            Method::GET,
            &format!("{SANDBOX_PATH}/{}", validate_sandbox_id(sandbox_id)?),
        )
        .send()
        .await
    }

    /// Update the name, subdomain, or lifetime of a Sandbox.
    pub async fn update(
        &self,
        sandbox_id: &str,
        request: UpdateSandboxRequest,
    ) -> Result<SandboxInfoResponse> {
        self.request(
            "update",
            Method::PATCH,
            &format!("{SANDBOX_PATH}/{}", validate_sandbox_id(sandbox_id)?),
        )
        .json(Some(&request))
        .send()
        .await
    }

    /// Stop and archive a Sandbox, retaining snapshots for later resume.
    pub async fn stop(
        &self,
        sandbox_id: &str,
        request: Option<StopRequest>,
    ) -> Result<SandboxActionResponse> {
        self.request(
            "stop",
            Method::POST,
            &format!("{SANDBOX_PATH}/{}/stop", validate_sandbox_id(sandbox_id)?),
        )
        .json(request.as_ref())
        .send()
        .await
    }

    /// Resume an archived Sandbox with optional settings.
    pub async fn resume(
        &self,
        sandbox_id: &str,
        request: Option<ResumeRequest>,
    ) -> Result<SandboxActionResponse> {
        self.request(
            "resume",
            Method::POST,
            &format!("{SANDBOX_PATH}/{}/resume", validate_sandbox_id(sandbox_id)?),
        )
        .json(request.as_ref())
        .send()
        .await
    }

    /// Permanently delete a sandbox and its exclusively-owned snapshot chains.
    /// The API requires the sandbox id again in the confirmation header.
    pub async fn delete_sandbox(&self, sandbox_id: &str) -> Result<DeletionOperationResponse> {
        self.request(
            "delete_sandbox",
            Method::DELETE,
            &format!("{SANDBOX_PATH}/{}", validate_sandbox_id(sandbox_id)?),
        )
        .header("X-Ascii-Confirm-Delete", Some(sandbox_id))
        .send()
        .await
    }

    /// Interrupt work running in a Sandbox.
    pub async fn interrupt(&self, sandbox_id: &str) -> Result<SandboxActionResponse> {
        self.interrupt_with_query(sandbox_id, None).await
    }

    /// Interrupt the selected conversation without automatically retrying delivery.
    pub async fn interrupt_with_query(
        &self,
        sandbox_id: &str,
        query: Option<&InterruptQuery>,
    ) -> Result<SandboxActionResponse> {
        self.request(
            "interrupt",
            Method::POST,
            &format!(
                "{SANDBOX_PATH}/{}/interrupt",
                validate_sandbox_id(sandbox_id)?
            ),
        )
        .query(query)
        .send()
        .await
    }

    // --- In-sandbox ops ---

    /// HTTP timeout is raised to cover `timeout_seconds` + slack. Commands are
    /// never auto-retried (Ascii: execution may already be running).
    ///
    pub async fn command(
        &self,
        sandbox_id: &str,
        request: CommandRequest,
    ) -> Result<CommandResponse> {
        if request.detached == Some(true) {
            return Err(Error::Config(
                "use command_raw for detached commands".into(),
            ));
        }
        match self.command_raw(sandbox_id, request).await? {
            CommandResult::Completed(response) => Ok(response),
            CommandResult::Started(_) => Err(Error::Unexpected(
                "detached response for synchronous command".into(),
            )),
        }
    }

    /// Start a JSON synchronous or detached command and preserve the union response.
    /// NDJSON streaming is rejected before delivery; detached commands ignore `stream`.
    pub async fn command_raw(
        &self,
        sandbox_id: &str,
        request: CommandRequest,
    ) -> Result<CommandResult> {
        if request.stream == Some(true) && request.detached != Some(true) {
            return Err(Error::Config(
                "NDJSON command streaming is unsupported by the JSON command API".into(),
            ));
        }
        self.request(
            "command",
            Method::POST,
            &format!(
                "{SANDBOX_PATH}/{}/commands",
                validate_sandbox_id(sandbox_id)?
            ),
        )
        .timeout(
            self.config
                .http_timeout_for_command(request.timeout_seconds),
        )
        .json(Some(&request))
        .send()
        .await
    }

    /// Read a detached command status and optionally limit its log tail.
    pub async fn command_status(
        &self,
        sandbox_id: &str,
        process_id: &str,
        query: Option<&CommandStatusQuery>,
    ) -> Result<CommandStatusResponse> {
        self.request(
            "command_status",
            Method::GET,
            &format!(
                "{SANDBOX_PATH}/{}/commands/{}",
                validate_sandbox_id(sandbox_id)?,
                segment(process_id)?
            ),
        )
        .query(query)
        .send()
        .await
    }

    /// Queue an agent prompt in a Sandbox.
    pub async fn prompt(&self, sandbox_id: &str, request: PromptRequest) -> Result<PromptResponse> {
        self.request(
            "prompt",
            Method::POST,
            &format!("{SANDBOX_PATH}/{}/prompt", validate_sandbox_id(sandbox_id)?),
        )
        .json(Some(&request))
        .send()
        .await
    }

    /// Read the state of a queued prompt.
    pub async fn prompt_run_status(
        &self,
        sandbox_id: &str,
        prompt_id: &str,
    ) -> Result<PromptRunResponse> {
        self.request(
            "prompt_run_status",
            Method::GET,
            &format!(
                "{SANDBOX_PATH}/{}/prompts/{}",
                validate_sandbox_id(sandbox_id)?,
                segment(prompt_id)?
            ),
        )
        .send()
        .await
    }

    /// Read a page of Sandbox events with optional cursor and type filters.
    pub async fn events(
        &self,
        sandbox_id: &str,
        query: Option<&EventsQuery>,
    ) -> Result<EventsResponse> {
        self.request(
            "events",
            Method::GET,
            &format!("{SANDBOX_PATH}/{}/events", validate_sandbox_id(sandbox_id)?),
        )
        .query(query)
        .send()
        .await
    }

    /// Request desktop access with connection and theme options.
    pub async fn desktop(
        &self,
        sandbox_id: &str,
        query: Option<&DesktopQuery>,
        request: Option<DesktopRequest>,
    ) -> Result<DesktopResponse> {
        self.request(
            "desktop",
            Method::POST,
            &format!(
                "{SANDBOX_PATH}/{}/desktop",
                validate_sandbox_id(sandbox_id)?
            ),
        )
        .json(request.as_ref())
        .query(query)
        .send()
        .await
    }

    // --- Snapshots ---
    /// List snapshot history for one Sandbox.
    pub async fn list_sandbox_snapshots(
        &self,
        sandbox_id: &str,
        query: Option<&SnapshotsQuery>,
    ) -> Result<SnapshotListResponse> {
        self.request(
            "list_sandbox_snapshots",
            Method::GET,
            &format!(
                "{SANDBOX_PATH}/{}/snapshots",
                validate_sandbox_id(sandbox_id)?
            ),
        )
        .query(query)
        .send()
        .await
    }
    /// Read the most recent snapshot for a Sandbox, if available.
    pub async fn latest_sandbox_snapshot(
        &self,
        sandbox_id: &str,
    ) -> Result<SnapshotLatestResponse> {
        self.request(
            "latest_sandbox_snapshot",
            Method::GET,
            &format!(
                "{SANDBOX_PATH}/{}/snapshots/latest",
                validate_sandbox_id(sandbox_id)?
            ),
        )
        .send()
        .await
    }
    /// List snapshots across the account with optional pagination.
    pub async fn list_snapshots(
        &self,
        query: Option<&SnapshotsQuery>,
    ) -> Result<SnapshotListResponse> {
        self.request("list_snapshots", Method::GET, "/snapshots")
            .query(query)
            .send()
            .await
    }
    /// Read the file tree and availability metadata of a snapshot.
    pub async fn snapshot_tree(&self, snapshot_id: &str) -> Result<SnapshotTreeResponse> {
        self.request(
            "snapshot_tree",
            Method::GET,
            &format!("/snapshots/{}/tree", segment(snapshot_id)?),
        )
        .send()
        .await
    }

    /// Download a snapshot file as bytes. `path` is a query value, not a URL segment.
    pub async fn snapshot_file(&self, snapshot_id: &str, path: Option<&str>) -> Result<Vec<u8>> {
        self.request(
            "snapshot_file",
            Method::GET,
            &format!("/snapshots/{}/files", segment(snapshot_id)?),
        )
        .query(path.map(|path| [("path", path)]).as_ref())
        .bytes()
        .await
    }

    /// Read a snapshot download manifest and reconstruction metadata.
    pub async fn snapshot_download(&self, snapshot_id: &str) -> Result<SnapshotDownloadResponse> {
        self.request(
            "snapshot_download",
            Method::GET,
            &format!("/snapshots/{}/download", segment(snapshot_id)?),
        )
        .send()
        .await
    }
    /// Accept permanent deletion of a snapshot and return its operation.
    pub async fn delete_snapshot(&self, snapshot_id: &str) -> Result<DeletionOperationResponse> {
        self.request(
            "delete_snapshot",
            Method::DELETE,
            &format!("/snapshots/{}", segment(snapshot_id)?),
        )
        .header("X-Ascii-Confirm-Delete", Some(snapshot_id))
        .send()
        .await
    }

    /// Read the current state of an accepted deletion operation.
    pub async fn get_deletion_operation(
        &self,
        operation_id: &str,
    ) -> Result<DeletionOperationResponse> {
        self.request(
            "get_deletion_operation",
            Method::GET,
            &format!("/deletion-operations/{}", segment(operation_id)?),
        )
        .send()
        .await
    }

    // --- Environments ---
    /// List configured environments.
    pub async fn environments(&self) -> Result<SandboxEnvironmentListResponse> {
        self.request("environments", Method::GET, "/environments")
            .send()
            .await
    }
    /// Create a named environment.
    pub async fn create_environment(
        &self,
        request: CreateSandboxEnvironmentRequest,
    ) -> Result<SandboxEnvironmentListResponse> {
        self.request("create_environment", Method::POST, "/environments")
            .json(Some(&request))
            .send()
            .await
    }
    /// Update environment contents, repository settings, or setup options.
    pub async fn update_environment(
        &self,
        id: &str,
        request: UpdateSandboxEnvironmentRequest,
    ) -> Result<SandboxEnvironmentResponse> {
        self.request(
            "update_environment",
            Method::PUT,
            &format!("/environments/{}", segment(id)?),
        )
        .json(Some(&request))
        .send()
        .await
    }
    /// Delete an environment.
    pub async fn delete_environment(&self, id: &str) -> Result<SandboxEnvironmentResponse> {
        self.request(
            "delete_environment",
            Method::DELETE,
            &format!("/environments/{}", segment(id)?),
        )
        .send()
        .await
    }

    /// Run a shell command and wait for its result, with a 30-second command timeout.
    pub async fn exec(
        &self,
        sandbox_id: &str,
        command: impl Into<String>,
    ) -> Result<CommandResponse> {
        self.command(
            sandbox_id,
            CommandRequest::new(command).with_timeout_seconds(30),
        )
        .await
    }

    /// Read a file with an optional encoding.
    pub async fn read_file(
        &self,
        sandbox_id: &str,
        path: impl Into<String>,
        encoding: Option<&str>,
    ) -> Result<FileReadResponse> {
        let path = path.into();
        validate_file_path(&path)?;
        self.request(
            "read_file",
            Method::GET,
            &format!("{SANDBOX_PATH}/{}/files", validate_sandbox_id(sandbox_id)?),
        )
        .query(Some(&FileReadQuery {
            path,
            encoding: encoding.map(str::to_owned),
        }))
        .send()
        .await
    }

    /// Write a file using the supplied path, content, and encoding.
    pub async fn write_file(
        &self,
        sandbox_id: &str,
        request: FileWriteRequest,
    ) -> Result<FileWriteResponse> {
        validate_file_path(&request.path)?;
        self.request(
            "write_file",
            Method::PUT,
            &format!("{SANDBOX_PATH}/{}/files", validate_sandbox_id(sandbox_id)?),
        )
        .json(Some(&request))
        .send()
        .await
    }

    /// Expose a port with explicit visibility and title options.
    pub async fn host_port(
        &self,
        sandbox_id: &str,
        request: HostPortRequest,
    ) -> Result<HostPortResponse> {
        self.request(
            "host_port",
            Method::POST,
            &format!("{SANDBOX_PATH}/{}/host", validate_sandbox_id(sandbox_id)?),
        )
        .json(Some(&request))
        .send()
        .await
    }

    /// Configure a public SSH key for a Sandbox.
    pub async fn ssh_key(
        &self,
        sandbox_id: &str,
        public_key: impl Into<String>,
    ) -> Result<SshKeyResponse> {
        self.request(
            "ssh_key",
            Method::POST,
            &format!("{SANDBOX_PATH}/{}/sshkey", validate_sandbox_id(sandbox_id)?),
        )
        .json(Some(&SshKeyRequest {
            public_key: public_key.into(),
        }))
        .send()
        .await
    }
}

pub(crate) fn validate_sandbox_id(sandbox_id: &str) -> Result<&str> {
    let rest = match sandbox_id.strip_prefix(SANDBOX_ID_PREFIX) {
        Some(r) if r.len() == SANDBOX_ID_BODY_LEN => r,
        _ => return Err(Error::InvalidSandboxId(sandbox_id.to_string())),
    };
    if !rest.bytes().all(|b| SANDBOX_ID_ALPHABET.contains(&b)) {
        return Err(Error::InvalidSandboxId(sandbox_id.to_string()));
    }
    Ok(sandbox_id)
}

fn validate_file_path(path: &str) -> Result<()> {
    if path.is_empty() || path.chars().all(char::is_whitespace) {
        return Err(Error::Config("file path must not be empty".into()));
    }
    Ok(())
}

fn segment(value: &str) -> Result<String> {
    if value.is_empty() || value.chars().all(char::is_whitespace) || matches!(value, "." | "..") {
        return Err(Error::Config(
            "path identifier must not be empty or a dot segment".into(),
        ));
    }
    Ok(utf8_percent_encode(value, PATH_SEGMENT).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_sandbox_id() {
        assert_eq!(validate_sandbox_id("bx_23456789").unwrap(), "bx_23456789");
        assert_eq!(validate_sandbox_id("bx_abcdefgh").unwrap(), "bx_abcdefgh");
    }

    #[test]
    fn rejects_bad_sandbox_id() {
        assert!(validate_sandbox_id("bx_SHORT").is_err());
        assert!(validate_sandbox_id("../etc/passwd").is_err());
        assert!(validate_sandbox_id("bx_AAAAAAAA").is_err());
        assert!(validate_sandbox_id("bx_23456789/../x").is_err());
    }
}
