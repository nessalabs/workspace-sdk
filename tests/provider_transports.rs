use async_trait::async_trait;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use workspace_sdk::supervisor::protocol::{
    SupervisorOperation, SupervisorRequest, SupervisorResponse, SupervisorValue,
};
use workspace_sdk::{providers::*, *};

#[derive(serde::Serialize)]
struct TestLocator<'a> {
    root: &'a str,
    control: &'a str,
    ownership: Ownership,
    target: &'a str,
}

#[derive(Default)]
struct RecordingRunner {
    requests: Mutex<Vec<NativeCommand>>,
    responses: Mutex<VecDeque<ExecResult>>,
}
#[async_trait]
impl ICommandRunner for RecordingRunner {
    async fn run(&self, command: NativeCommand) -> Result<ExecResult> {
        let fallback = serde_json::from_slice::<SupervisorRequest>(&command.stdin)
            .map(|request| match request.operation {
                SupervisorOperation::Start { .. } => SupervisorValue::Run(RunState::Running),
                _ => SupervisorValue::Empty,
            })
            .unwrap_or(SupervisorValue::Empty);
        self.requests.lock().unwrap().push(command);
        Ok(self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| {
                result(
                    0,
                    &serde_json::to_string(&SupervisorResponse::Success(fallback)).unwrap(),
                )
            }))
    }
}
fn result(code: i32, stdout: &str) -> ExecResult {
    ExecResult {
        exit_code: Some(code),
        stdout: stdout.into(),
        stderr: String::new(),
        timed_out: false,
        output_truncated: false,
    }
}
fn request(id: &str) -> OpenRequest {
    OpenRequest {
        workspace_id: WorkspaceId::new(id).unwrap(),
        config: ProviderOptions::default(),
        resources: ResourceRequirements::default(),
    }
}
#[tokio::test]
async fn ssh_verifies_known_hosts_and_puts_secrets_on_stdin() {
    let runner = Arc::new(RecordingRunner::default());
    let provider = SshProvider::with_runner(
        SshConfig::trusted_host("gpu-worker", "/srv/workspaces"),
        runner.clone(),
    )
    .unwrap();
    let backend = provider.open(request("experiment")).await.unwrap();
    let mut secrets = SecretEnvironment::default();
    secrets
        .0
        .insert("KEY".into(), "secret-must-not-be-an-argument".into());
    backend
        .start(LaunchRequest {
            id: RunId::generate(),
            execution: ExecutionRequest {
                command: CommandSpec::new("model-runner"),
                secrets,
                private_stdin: None,
            },
            output_limit_bytes: 1024,
        })
        .await
        .unwrap();
    let calls = runner.requests.lock().unwrap();
    assert_eq!(calls.len(), 2);
    for call in calls.iter() {
        assert!(call.args.contains(&"StrictHostKeyChecking=yes".into()));
        assert!(call.args.contains(&"BatchMode=yes".into()));
        assert!(!format!("{:?}", call.args).contains("secret-must-not-be-an-argument"));
        assert!(!format!("{call:?}").contains("secret-must-not-be-an-argument"));
    }
    assert!(String::from_utf8_lossy(&calls[1].stdin).contains("secret-must-not-be-an-argument"));
    assert_eq!(backend.descriptor().isolation, Isolation::TrustedHost);
}
#[tokio::test]
async fn container_preflight_does_not_create_when_engine_is_missing() {
    let provider = ContainerProvider::new(ContainerConfig::new(
        "/nonexistent/workspace-sdk-container-engine",
        "debian:bookworm-slim",
        "/workspace",
    ))
    .unwrap();
    assert!(matches!(
        provider.open(request("missing-engine")).await,
        Err(Error::NotFound(_))
    ));
}
#[tokio::test]
async fn container_rejects_collision_with_unowned_existing_container() {
    let runner = Arc::new(RecordingRunner::default());
    runner.responses.lock().unwrap().extend([
        result(0, "{}"),
        result(
            0,
            r#"{"Config":{"Labels":{"another":"app"}},"State":{"Running":true}}"#,
        ),
    ]);
    let provider = ContainerProvider::with_runner(
        ContainerConfig::new("docker", "debian:bookworm-slim", "/workspace"),
        runner.clone(),
    )
    .unwrap();
    assert!(matches!(
        provider.open(request("collision")).await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(runner.requests.lock().unwrap().len(), 2);
}
#[tokio::test]
async fn attached_container_lifecycle_is_not_mutated() {
    let runner = Arc::new(RecordingRunner::default());
    runner.responses.lock().unwrap().extend([
        result(0, "{}"),
        result(0, r#"{"State":{"Running":true}}"#),
        result(0, r#"{"State":{"Running":true}}"#),
    ]);
    let provider = ContainerProvider::with_runner(
        ContainerConfig::new("podman", "debian:bookworm-slim", "/workspace"),
        runner.clone(),
    )
    .unwrap();
    let mut open = request("attached");
    open.config = ProviderOptions::from_options(&ContainerOpenOptions {
        container_id: Some("user-container".into()),
    })
    .unwrap();
    let backend = provider.open(open).await.unwrap();
    assert!(matches!(backend.stop().await, Err(Error::Unsupported(_))));
    assert!(matches!(
        backend.destroy().await,
        Err(Error::Unsupported(_))
    ));
    assert_eq!(backend.descriptor().isolation, Isolation::Container);
    let calls = runner.requests.lock().unwrap();
    assert!(!calls.iter().any(|call| call
        .args
        .first()
        .is_some_and(|arg| ["run", "start", "stop", "rm"].contains(&arg.as_str()))));
}

#[tokio::test]
async fn container_deletion_receipt_reconnects_and_finishes_a_remaining_volume() {
    let runner = Arc::new(RecordingRunner::default());
    let reference = WorkspaceRef {
        version: 1,
        id: WorkspaceId::new("partial").unwrap(),
        provider: ProviderId::new("container").unwrap(),
        locator: WorkspaceLocator::from_locator(&TestLocator {
            root: "/workspace/partial/project",
            control: "/workspace/partial/control",
            ownership: Ownership::SdkOwned,
            target: "workspace-sdk-partial",
        })
        .unwrap(),
    };
    let container = r#"{"Config":{"Labels":{"dev.workspace-sdk.workspace":"partial"}},"Mounts":[{"Type":"volume","Destination":"/workspace","Name":"workspace-sdk-partial"}]}"#;
    let volume = r#"{"Labels":{"dev.workspace-sdk.workspace":"partial"}}"#;
    runner.responses.lock().unwrap().extend([
        result(0, "workspace-sdk-partial\n"),
        result(0, "workspace-sdk-partial\n"),
        result(0, container),
        result(0, volume),
        result(0, "removed container"),
        result(1, ""),
        // A new controller can inspect the same receipt after container removal.
        result(0, ""),
        result(0, "workspace-sdk-partial\n"),
        // Retry only removes the still-owned volume and confirms both absences.
        result(0, ""),
        result(0, "workspace-sdk-partial\n"),
        result(0, volume),
        result(0, "removed volume"),
        result(0, ""),
        result(0, ""),
    ]);
    let provider = ContainerProvider::with_runner(
        ContainerConfig::new("docker", "debian:bookworm-slim", "/workspace"),
        runner.clone(),
    )
    .unwrap();
    let backend = provider.connect(&reference).await.unwrap();
    let partial = backend.destroy().await.unwrap();
    assert_eq!(partial.state, DeleteState::Blocked);
    assert!(partial
        .detail
        .as_deref()
        .unwrap()
        .contains("volume remains"));
    drop(backend);
    let reconnected = provider.connect(&reference).await.unwrap();
    assert_eq!(
        reconnected
            .inspect_delete(&partial.reference)
            .await
            .unwrap()
            .state,
        DeleteState::Blocked
    );
    let finished = reconnected.destroy().await.unwrap();
    assert_eq!(finished.state, DeleteState::Completed);
    assert_eq!(finished.reference.id, partial.reference.id);
    let calls = runner.requests.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.args.first().is_some_and(|arg| arg == "rm"))
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.args.starts_with(&["volume".into(), "rm".into()]))
            .count(),
        2
    );
    assert!(runner.responses.lock().unwrap().is_empty());
}

#[tokio::test]
async fn container_creation_refuses_to_reuse_an_unowned_volume() {
    let runner = Arc::new(RecordingRunner::default());
    runner.responses.lock().unwrap().extend([
        result(0, "{}"),
        result(1, ""),
        result(0, r#"{"Labels":{"another":"app"}}"#),
    ]);
    let provider = ContainerProvider::with_runner(
        ContainerConfig::new("docker", "debian:bookworm-slim", "/workspace"),
        runner.clone(),
    )
    .unwrap();
    assert!(matches!(
        provider.open(request("volume-collision")).await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(runner.requests.lock().unwrap().len(), 3);
}
