use super::{deletion::deletion, error::error};
use crate::providers::{
    managed::{self, ITarget, Locator},
    SupervisorDeployment,
};
use crate::{
    domain::*,
    supervisor::protocol::{SupervisorOperation, SupervisorRequest, SupervisorValue},
    Error, Result,
};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};
use sandbox_client::{
    wait_until_ready_with, write_text, Api, CommandResponse, FileWriteRequest, HostPortRequest,
    SandboxState, WaitOptions,
};
use serde::Deserialize;
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SandboxOwnerRecord {
    version: u32,
    workspace_id: WorkspaceId,
    root: String,
    control: String,
    ownership: Ownership,
}
pub(super) struct SandboxTarget {
    pub(super) api: Api,
    pub(super) supervisor: SupervisorDeployment,
    pub(super) locator: Locator,
    pub(super) workspace_id: WorkspaceId,
    pub(super) verified_owned: AtomicBool,
}
impl SandboxTarget {
    async fn native(&self, command: String) -> Result<CommandResponse> {
        let result = self
            .api
            .exec(&self.locator.target, command)
            .await
            .map_err(error)?;
        if result.stdout_truncated == Some(true) || result.stderr_truncated == Some(true) {
            return Err(Error::State(
                "Sandbox truncated a native response; request a smaller file or output page".into(),
            ));
        }
        Ok(result)
    }
    async fn native_success(&self, command: String) -> Result<CommandResponse> {
        let result = self.native(command).await?;
        if !result.success || result.timed_out || result.exit_code != Some(0) {
            return Err(Error::State(
                "Sandbox native operation failed (raw output redacted)".into(),
            ));
        }
        Ok(result)
    }
    async fn invoke_inline(&self, request: &SupervisorRequest) -> Result<SupervisorValue> {
        let text = serde_json::to_string(request)?;
        let result = self
            .native(format!(
                "printf '%s' {} | {}",
                managed::quote(&text),
                managed::quote(self.supervisor.executable())
            ))
            .await?;
        managed::response(
            &result.stdout,
            result.success && !result.timed_out && result.exit_code == Some(0),
        )
    }
    async fn verify_ownership(&self) -> Result<()> {
        if self.locator.ownership != Ownership::SdkOwned {
            return Err(Error::Unsupported(
                "attached Sandbox lifecycle cannot be mutated".into(),
            ));
        }
        if let Ok(file) = self
            .api
            .read_file(
                &self.locator.target,
                &format!("{}/owner.json", self.locator.control),
                Some("base64"),
            )
            .await
        {
            if file.ok {
                let bytes = STANDARD
                    .decode(file.content.as_deref().unwrap_or(""))
                    .map_err(|_| {
                        Error::State("Boat ownership marker is not valid base64".into())
                    })?;
                if bytes.len() > 4096 {
                    return Err(Error::State(
                        "Boat ownership marker exceeds its size limit".into(),
                    ));
                }
                let owner: SandboxOwnerRecord = serde_json::from_slice(&bytes)
                    .map_err(|_| Error::State("Boat ownership marker is malformed".into()))?;
                if owner.version != 1
                    || owner.workspace_id != self.workspace_id
                    || owner.root != self.locator.root
                    || owner.control != self.locator.control
                    || owner.ownership != Ownership::SdkOwned
                {
                    return Err(Error::Conflict(
                        "Sandbox owner marker does not match this SDK workspace".into(),
                    ));
                }
                self.verified_owned.store(true, Ordering::Release);
                return Ok(());
            }
        }
        if self.verified_owned.load(Ordering::Acquire) {
            let state = self
                .api
                .get(&self.locator.target)
                .await
                .map_err(error)?
                .sandbox
                .state;
            if matches!(state, SandboxState::Archived | SandboxState::Archiving) {
                return Ok(());
            }
        }
        Err(Error::State("Boat ownership cannot be verified; if archived, resume it through the provider and reconnect before lifecycle mutation".into()))
    }
    async fn install_supervisor(&self) -> Result<()> {
        if let SupervisorDeployment::Upload {
            artifact,
            executable,
        } = &self.supervisor
        {
            let platform = self.native_success("uname -sm".into()).await?;
            artifact.platform.verify_uname(&platform.stdout)?;
            let bytes = artifact.bytes()?;
            let parent = Path::new(executable)
                .parent()
                .and_then(|path| path.to_str())
                .ok_or_else(|| {
                    Error::InvalidInput("supervisor executable needs an absolute parent".into())
                })?;
            let upload = format!("{parent}/.supervisor-upload-{}", Uuid::new_v4());
            let temporary = format!("{executable}.pending-{}", Uuid::new_v4());
            self.native_success(format!(
                "umask 077; mkdir -p {} && chmod 700 {}",
                managed::quote(&upload),
                managed::quote(&upload)
            ))
            .await?;
            for (index, chunk) in bytes.chunks(256 * 1024).enumerate() {
                let response = self
                    .api
                    .write_file(
                        &self.locator.target,
                        FileWriteRequest {
                            path: format!("{upload}/part-{index:06}"),
                            content: STANDARD.encode(chunk),
                            encoding: Some("base64".into()),
                        },
                    )
                    .await
                    .map_err(error)?;
                if !response.ok || response.success == Some(false) {
                    return Err(Error::State(
                        "Sandbox supervisor artifact chunk upload failed".into(),
                    ));
                }
            }
            let command = format!(
                "umask 077; cat {}/* > {} && chmod 700 {} && mv -f {} {} && rm -rf {}",
                managed::quote(&upload),
                managed::quote(&temporary),
                managed::quote(&temporary),
                managed::quote(&temporary),
                managed::quote(executable),
                managed::quote(&upload)
            );
            self.native_success(command).await?;
        }
        Ok(())
    }
}
#[async_trait]
impl ITarget for SandboxTarget {
    async fn invoke(&self, request: SupervisorRequest) -> Result<SupervisorValue> {
        let sensitive = matches!(
            request.operation,
            SupervisorOperation::Start { .. }
                | SupervisorOperation::WriteFile { .. }
                | SupervisorOperation::WritePrivateFile { .. }
        );
        if !sensitive {
            return self.invoke_inline(&request).await;
        }
        // Credential bytes cross only the private file API. The Rust target
        // validates the directory before upload and unlinks before decoding.
        let id = Uuid::new_v4();
        let path = format!("{}/requests/{id}.json", self.locator.control);
        let prepare = SupervisorRequest::new(
            request.context.clone(),
            SupervisorOperation::PrepareRequest { id },
        );
        let cleanup = SupervisorRequest::new(
            request.context.clone(),
            SupervisorOperation::RemoveRequest { id },
        );
        let result = async {
            self.invoke_inline(&prepare).await?.empty()?;
            let written = write_text(
                &self.api,
                &self.locator.target,
                &path,
                serde_json::to_string(&request)?,
            )
            .await
            .map_err(error)?;
            if !written.ok || written.success == Some(false) {
                return Err(Error::State(
                    "Sandbox rejected the private request file".into(),
                ));
            }
            let result = self
                .native(format!(
                    "{} --request-file {}",
                    managed::quote(self.supervisor.executable()),
                    managed::quote(&path)
                ))
                .await?;
            managed::response(
                &result.stdout,
                result.success && !result.timed_out && result.exit_code == Some(0),
            )
        }
        .await;
        // A transport failure may occur before the executable consumes its
        // input. Cleanup is scoped to this UUID and never replaces the result.
        let _ = self.invoke_inline(&cleanup).await;
        result
    }
    async fn ensure(&self) -> Result<()> {
        let info = self.api.get(&self.locator.target).await.map_err(error)?;
        if info.sandbox.state == SandboxState::Archived {
            if self.locator.ownership == Ownership::Attached {
                return Err(Error::State(
                    "attached Sandbox is archived; its owner must resume it".into(),
                ));
            }
            self.verify_ownership().await?;
            self.api
                .resume(&self.locator.target, None)
                .await
                .map_err(error)?;
        }
        wait_until_ready_with(
            &self.api,
            &self.locator.target,
            WaitOptions::from_millis(180_000, 1_000),
        )
        .await
        .map_err(error)?;
        self.install_supervisor().await
    }
    async fn state(&self) -> Result<WorkspaceState> {
        match self.api.get(&self.locator.target).await {
            Ok(info) => Ok(match info.sandbox.state {
                state if state.is_operable() => WorkspaceState::Ready,
                SandboxState::Archived | SandboxState::Archiving => WorkspaceState::Stopped,
                SandboxState::Init
                | SandboxState::Provisioning
                | SandboxState::Provisioned
                | SandboxState::Cloning => WorkspaceState::Provisioning,
                _ => WorkspaceState::Unknown,
            }),
            Err(error) if error.status() == Some(404) => Ok(WorkspaceState::Unknown),
            Err(failure) => Err(error(failure)),
        }
    }
    async fn port(&self, request: PortRequest) -> Result<PortAccess> {
        if request.policy != PortPolicy::Authenticated {
            return Err(Error::Unsupported(
                "Sandbox private port tunneling; use authenticated hosting".into(),
            ));
        }
        let response = self
            .api
            .host_port(
                &self.locator.target,
                HostPortRequest {
                    port: request.port,
                    public: Some(false),
                    localhost: None,
                    title: None,
                },
            )
            .await
            .map_err(error)?;
        if response.is_protected != Some(true) {
            return Err(Error::State(
                "Sandbox did not confirm protected hosting".into(),
            ));
        }
        let url = response
            .url
            .ok_or_else(|| Error::State("Sandbox did not return a protected port URL".into()))?;
        if !url.starts_with("https://") {
            return Err(Error::State("Sandbox protected URL must use HTTPS".into()));
        }
        Ok(PortAccess {
            url,
            policy: PortPolicy::Authenticated,
        })
    }
    async fn stop(&self) -> Result<()> {
        self.verify_ownership().await?;
        self.api
            .stop(&self.locator.target, None)
            .await
            .map_err(error)?;
        Ok(())
    }
    async fn destroy(&self, reference: &WorkspaceRef) -> Result<DeleteOperation> {
        self.verify_ownership().await?;
        deletion(
            reference,
            self.api
                .delete_sandbox(&self.locator.target)
                .await
                .map_err(error)?
                .operation,
        )
    }
    async fn inspect_delete(&self, reference: &OperationRef) -> Result<DeleteOperation> {
        let response = self
            .api
            .get_deletion_operation(reference.id.as_str())
            .await
            .map_err(error)?;
        if response.operation.target_id != self.locator.target {
            return Err(Error::InvalidInput(
                "Sandbox deletion operation belongs to another target".into(),
            ));
        }
        deletion(&reference.workspace, response.operation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supervisor::protocol::{
        SupervisorContext, SupervisorExecution, SupervisorResponse, WorkingDirectory,
    };
    use sandbox_client::ClientConfig;
    use serde::{Deserialize, Serialize};
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
        thread::{self, JoinHandle},
        time::Duration,
    };

    struct HttpRequest {
        method: String,
        route: String,
        body: Vec<u8>,
    }
    #[derive(Deserialize)]
    struct WireCommand {
        command: String,
    }
    #[derive(Deserialize)]
    struct WireFile {
        path: String,
        content: String,
        encoding: String,
    }
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct CommandReply {
        ok: bool,
        success: bool,
        exit_code: i32,
        stdout: String,
        stderr: String,
        timed_out: bool,
    }
    fn command_reply(success: bool, value: SupervisorValue) -> String {
        serde_json::to_string(&CommandReply {
            ok: true,
            success,
            exit_code: if success { 0 } else { 1 },
            stdout: serde_json::to_string(&SupervisorResponse::Success(value)).unwrap(),
            stderr: String::new(),
            timed_out: false,
        })
        .unwrap()
    }
    fn server(replies: Vec<String>) -> (SandboxTarget, JoinHandle<Vec<HttpRequest>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let mut requests = vec![];
            for reply in replies {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                let fields = first.split_whitespace().collect::<Vec<_>>();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':') {
                        if name.eq_ignore_ascii_case("content-length") {
                            length = value.trim().parse::<usize>().unwrap();
                        }
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                requests.push(HttpRequest {
                    method: fields[0].into(),
                    route: fields[1].into(),
                    body,
                });
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", reply.len(), reply).unwrap();
            }
            requests
        });
        let config = ClientConfig::new("test-api-key")
            .unwrap()
            .with_base_path(format!("http://{address}"))
            .unwrap();
        let target = SandboxTarget {
            api: Api::new(config).unwrap(),
            supervisor: SupervisorDeployment::Preinstalled {
                executable: "/opt/sdk/workspace-supervisor".into(),
            },
            locator: Locator {
                root: "/srv/workspaces/test/project".into(),
                control: "/srv/workspaces/test/control".into(),
                ownership: Ownership::SdkOwned,
                target: "bx_23456789".into(),
            },
            workspace_id: WorkspaceId::new("test").unwrap(),
            verified_owned: AtomicBool::new(false),
        };
        (target, worker)
    }
    fn launch(target: &SandboxTarget) -> SupervisorRequest {
        let mut secrets = SecretEnvironment::default();
        secrets
            .0
            .insert("API_TOKEN".into(), "only-in-private-input".into());
        SupervisorRequest::new(
            SupervisorContext {
                workspace_id: target.workspace_id.clone(),
                root: target.locator.root.clone(),
                control: target.locator.control.clone(),
                ownership: Ownership::SdkOwned,
            },
            SupervisorOperation::Start {
                id: RunId::new("run-one").unwrap(),
                execution: Box::new(SupervisorExecution {
                    command: CommandSpec::new("true"),
                    secrets,
                    private_stdin: None,
                    directory: WorkingDirectory::Project(WorkspacePath::root()),
                }),
                output_limit_bytes: 4096,
            },
        )
    }
    fn command(request: &HttpRequest) -> WireCommand {
        assert_eq!(request.method, "POST");
        assert_eq!(request.route, "/sandboxes/bx_23456789/commands");
        serde_json::from_slice(&request.body).unwrap()
    }

    #[tokio::test]
    async fn hosting_explicitly_requests_private_access() {
        let (target, worker) = server(vec![
            r#"{"ok":true,"isProtected":true,"url":"https://example.test/fixture"}"#.into(),
        ]);
        let access = target
            .port(PortRequest {
                port: 8080,
                policy: PortPolicy::Authenticated,
            })
            .await
            .unwrap();
        assert_eq!(access.policy, PortPolicy::Authenticated);
        let requests = worker.join().unwrap();
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].route, "/sandboxes/bx_23456789/host");
        let request: HostPortRequest = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(request.port, 8080);
        assert_eq!(request.public, Some(false));
    }

    #[tokio::test]
    async fn launches_use_private_put_and_cleanup_after_consumption() {
        let (target, worker) = server(vec![
            command_reply(true, SupervisorValue::Empty),
            r#"{"ok":true,"success":true}"#.into(),
            command_reply(true, SupervisorValue::Run(RunState::Starting)),
            command_reply(true, SupervisorValue::Empty),
        ]);
        assert_eq!(
            target.invoke(launch(&target)).await.unwrap().run().unwrap(),
            RunState::Starting
        );
        let requests = worker.join().unwrap();
        assert!(command(&requests[0]).command.contains("prepare_request"));
        assert_eq!(requests[1].method, "PUT");
        assert_eq!(requests[1].route, "/sandboxes/bx_23456789/files");
        let file: WireFile = serde_json::from_slice(&requests[1].body).unwrap();
        assert_eq!(file.encoding, "utf8");
        assert!(file
            .path
            .starts_with("/srv/workspaces/test/control/requests/"));
        let payload: SupervisorRequest = serde_json::from_str(&file.content).unwrap();
        let SupervisorOperation::Start { execution, .. } = payload.operation else {
            panic!("expected typed launch");
        };
        assert_eq!(execution.secrets.0["API_TOKEN"], "only-in-private-input");
        assert!(command(&requests[2]).command.contains("--request-file"));
        assert!(command(&requests[2]).command.contains(&file.path));
        assert!(command(&requests[3]).command.contains("remove_request"));
        for index in [0, 2, 3] {
            assert!(!command(&requests[index])
                .command
                .contains("only-in-private-input"));
        }
    }

    #[tokio::test]
    async fn rejected_native_launch_still_cleans_the_private_request() {
        let (target, worker) = server(vec![
            command_reply(true, SupervisorValue::Empty),
            r#"{"ok":true,"success":true}"#.into(),
            command_reply(false, SupervisorValue::Empty),
            command_reply(true, SupervisorValue::Empty),
        ]);
        assert!(target.invoke(launch(&target)).await.is_err());
        let requests = worker.join().unwrap();
        assert!(command(&requests[3]).command.contains("remove_request"));
    }

    #[tokio::test]
    async fn rejected_upload_still_cleans_its_reserved_request() {
        let (target, worker) = server(vec![
            command_reply(true, SupervisorValue::Empty),
            r#"{"ok":false,"success":false}"#.into(),
            command_reply(true, SupervisorValue::Empty),
        ]);
        assert!(target.invoke(launch(&target)).await.is_err());
        let requests = worker.join().unwrap();
        assert!(command(&requests[2]).command.contains("remove_request"));
    }
}
