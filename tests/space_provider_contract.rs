#![cfg(feature = "space")]
//! Isolated pinned SPACE protocol qualification with real native supervisor
//! execution. These tests neither provision a VM nor qualify live SPACE/KVM.
#[path = "support/space_http_fixture.rs"]
mod fixture;
mod support;
use fixture::{Fault, Fixture, ATTACH, MANAGEMENT, SANDBOX, USER};
use std::{fs, sync::Arc, time::Duration};
use workspace_sdk::{
    providers::{SpaceConfig, SpaceOpenOptions, SpaceProvider},
    *,
};

fn temporary() -> fixture::NativeDirectory {
    fixture::NativeDirectory::new()
}
fn provider(fixture: &Fixture) -> SpaceProvider {
    SpaceProvider::new(
        SpaceConfig::new(
            &fixture.api_url,
            &fixture.data_url,
            SecretInput::new(MANAGEMENT.as_bytes().to_vec()),
            USER,
            env!("CARGO_BIN_EXE_workspace-supervisor"),
        )
        .unwrap()
        .with_rpc_timeout(Duration::from_secs(3))
        .unwrap(),
    )
}
async fn open(fixture: &Fixture, root: &std::path::Path) -> Arc<dyn IBackend> {
    fs::create_dir_all(root).unwrap();
    provider(fixture)
        .open(
            OpenRequest::new(
                WorkspaceId::generate(),
                &SpaceOpenOptions::new(SANDBOX, root.to_str().unwrap()),
            )
            .unwrap(),
        )
        .await
        .unwrap()
}
fn launch(id: RunId, arguments: &[&str]) -> LaunchRequest {
    LaunchRequest {
        id,
        execution: CommandSpec::new(support::workload_program())
            .args(arguments.iter().copied())
            .into(),
        output_limit_bytes: 4096,
    }
}
async fn terminal(backend: &dyn IBackend, run: &RunRef) -> RunState {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let state = backend.inspect_run(run).await.unwrap();
            if !state.is_active() {
                return state;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap()
}

#[test]
fn explicit_config_refuses_untrusted_origins_and_redacts_credentials() {
    for endpoint in [
        "http://example.com",
        "https://user:password@example.com",
        "https://example.com?token=x",
        "https://example.com/#fragment",
        "file:///tmp/space",
        "http://127.0.0.1.evil.test",
    ] {
        let config = SpaceConfig::new(
            endpoint,
            "https://data.example.com",
            SecretInput::new(MANAGEMENT.as_bytes().to_vec()),
            USER,
            "/opt/sdk/workspace-supervisor",
        );
        assert!(
            matches!(config, Err(Error::InvalidInput(_))),
            "accepted endpoint {endpoint}"
        );
    }
    let config = SpaceConfig::new(
        "https://api.example.com",
        "https://data.example.com",
        SecretInput::new(MANAGEMENT.as_bytes().to_vec()),
        USER,
        "/opt/sdk/workspace-supervisor",
    )
    .unwrap();
    assert!(!format!("{config:?}").contains(MANAGEMENT));
    assert!(config.clone().with_rpc_timeout(Duration::ZERO).is_err());
    assert!(config.with_rpc_timeout(Duration::from_secs(121)).is_err());
}

#[tokio::test]
async fn invalid_options_and_resources_fail_before_any_network_request() {
    let fixture = Fixture::new();
    for root in [
        "relative",
        "/",
        "/tmp/../project",
        "/tmp//project",
        "/tmp/project/",
        "/tmp/.workspace-sdk/project",
        "/tmp/.WORKSPACE-SDK/project",
    ] {
        let request = OpenRequest::new(
            WorkspaceId::generate(),
            &SpaceOpenOptions::new(SANDBOX, root),
        )
        .unwrap();
        assert!(matches!(
            provider(&fixture).open(request).await,
            Err(Error::InvalidInput(_))
        ));
    }
    for sandbox in ["", "bx_old", "sbx_../escape"] {
        let request = OpenRequest::new(
            WorkspaceId::generate(),
            &SpaceOpenOptions::new(sandbox, "/tmp/project"),
        )
        .unwrap();
        assert!(matches!(
            provider(&fixture).open(request).await,
            Err(Error::InvalidInput(_))
        ));
    }
    let mut request = OpenRequest::new(
        WorkspaceId::generate(),
        &SpaceOpenOptions::new(SANDBOX, "/tmp/project"),
    )
    .unwrap();
    request.resources.cpu = Some(2);
    assert!(matches!(
        provider(&fixture).open(request).await,
        Err(Error::Unsupported(_))
    ));
    assert!(fixture.requests().is_empty());
}

#[tokio::test]
async fn native_claim_reconnect_output_files_tree_and_attached_lifecycle() {
    let directory = temporary();
    let fixture = Fixture::new();
    let backend = open(&fixture, &directory.path().join("project")).await;
    let descriptor = backend.descriptor();
    assert_eq!(descriptor.ownership, Ownership::Attached);
    assert_eq!(
        std::path::Path::new(&descriptor.control_root),
        directory
            .path()
            .join(".workspace-sdk")
            .join(descriptor.reference.id.as_str())
    );
    assert!(!descriptor.capabilities.stop && !descriptor.capabilities.destroy);
    let serialized = serde_json::to_string(&descriptor.reference).unwrap();
    assert!(!serialized.contains(MANAGEMENT) && !serialized.contains(ATTACH));
    assert!(matches!(backend.stop().await, Err(Error::Unsupported(_))));
    assert!(matches!(
        backend.destroy().await,
        Err(Error::Unsupported(_))
    ));
    let id = RunId::generate();
    let arguments = ["append", "launched", "once\n", "0.1", "0", "done"];
    let run = backend.start(launch(id.clone(), &arguments)).await.unwrap();
    assert_eq!(
        terminal(backend.as_ref(), &run).await,
        RunState::Exited { exit_code: 0 }
    );
    drop(backend);
    let before = fixture.requests().len();
    let backend = provider(&fixture)
        .connect(&descriptor.reference)
        .await
        .unwrap();
    backend.start(launch(id.clone(), &arguments)).await.unwrap();
    assert_eq!(
        fs::read_to_string(directory.path().join("project/launched")).unwrap(),
        "once\n"
    );
    assert!(matches!(
        backend.start(launch(id, &["echo", "different"])).await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(
        backend
            .read_output(&run, OutputCursor::default(), 1024)
            .await
            .unwrap()
            .stdout,
        "done\n"
    );
    let path = WorkspacePath::new("binary").unwrap();
    backend
        .write_file(WriteFile {
            path: path.clone(),
            offset: 0,
            bytes: vec![0, 255, 1],
            truncate: true,
            executable: false,
        })
        .await
        .unwrap();
    backend
        .write_file(WriteFile {
            path: path.clone(),
            offset: 2,
            bytes: vec![7, 8],
            truncate: false,
            executable: false,
        })
        .await
        .unwrap();
    assert_eq!(
        backend
            .read_file(ReadFile {
                path,
                offset: 1,
                limit: 3
            })
            .await
            .unwrap()
            .bytes,
        vec![255, 7, 8]
    );
    let full_chunk: Vec<u8> = (0..256 * 1024).map(|index| (index % 251) as u8).collect();
    let full_path = WorkspacePath::new("full-binary-chunk").unwrap();
    backend
        .write_file(WriteFile {
            path: full_path.clone(),
            offset: 0,
            bytes: full_chunk.clone(),
            truncate: true,
            executable: false,
        })
        .await
        .unwrap();
    assert_eq!(
        backend
            .read_file(ReadFile {
                path: full_path,
                offset: 0,
                limit: 256 * 1024
            })
            .await
            .unwrap()
            .bytes,
        full_chunk
    );
    let transfer = TreeTransfer::new();
    let exported = backend.export_tree(&transfer).await.unwrap();
    assert!(exported.bytes > 0 && exported.files >= 2);
    backend.remove_transfer(&transfer).await.unwrap();
    // The reconnect sequence must probe existing identity, never initialize it.
    let requests = fixture.requests();
    for request in &requests[before..] {
        if let Some(encoded) = request.body["command"][4].as_str() {
            if let Ok(native) = serde_json::from_str::<serde_json::Value>(encoded) {
                assert_ne!(
                    native["operation"], "initialize",
                    "connect reinitialized native identity"
                );
            }
        }
    }
    fixture.assert_valid();
}

#[tokio::test]
async fn lost_launch_ack_reconciles_same_native_claim_without_duplicate_work() {
    let directory = temporary();
    let fixture = Fixture::new();
    let backend = open(&fixture, &directory.path().join("project")).await;
    let id = RunId::generate();
    let arguments = ["append", "launched", "once\n", "0.1", "0", "done"];
    fixture.fault(Fault::LaunchAckLoss);
    assert!(backend.start(launch(id.clone(), &arguments)).await.is_err());
    let run = backend.start(launch(id, &arguments)).await.unwrap();
    assert_eq!(
        terminal(backend.as_ref(), &run).await,
        RunState::Exited { exit_code: 0 }
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("project/launched")).unwrap(),
        "once\n"
    );
    fixture.assert_valid();
}

#[tokio::test]
async fn private_upload_preserves_native_permissions_and_secret_boundary() {
    let directory = temporary();
    let fixture = Fixture::new();
    let backend = open(&fixture, &directory.path().join("project")).await;
    let secret = "private-payload-never-in-command-record";
    let mut request: ExecutionRequest = CommandSpec::new("/bin/sh")
        .args(["-c", "test -n \"$PRIVATE_VALUE\""])
        .into();
    request
        .secrets
        .0
        .insert("PRIVATE_VALUE".into(), secret.into());
    let result = backend.exec(request).await.unwrap();
    assert_eq!(result.exit_code, Some(0));
    let requests = fixture.requests();
    let uploads: Vec<_> = requests
        .iter()
        .filter(|request| request.path.ends_with("/files:write"))
        .collect();
    assert_eq!(uploads.len(), 1);
    let commands: Vec<_> = requests
        .iter()
        .filter(|request| request.path.ends_with("/commands"))
        .map(|request| &request.body)
        .collect();
    assert!(!serde_json::to_string(&commands).unwrap().contains(secret));
    assert!(!serde_json::to_string(&backend.descriptor().reference)
        .unwrap()
        .contains(secret));
    assert!(!std::path::Path::new(uploads[0].body["path"].as_str().unwrap()).exists());
    fixture.assert_valid();
}

#[tokio::test]
async fn ambiguous_or_short_upload_is_never_invoked_or_replayed() {
    for fault in [Fault::ShortUpload, Fault::UploadAckLoss] {
        let directory = temporary();
        let fixture = Fixture::new();
        let backend = open(&fixture, &directory.path().join("project")).await;
        fixture.fault(fault);
        let mut request: ExecutionRequest = CommandSpec::new("/bin/sh")
            .args(["-c", "touch should-not-run"])
            .into();
        request
            .secrets
            .0
            .insert("PRIVATE_VALUE".into(), "secret".into());
        assert!(backend.exec(request).await.is_err());
        assert!(!directory.path().join("project/should-not-run").exists());
        let requests = fixture.requests();
        let uploads: Vec<_> = requests
            .iter()
            .filter(|request| request.path.ends_with("/files:write"))
            .collect();
        assert_eq!(uploads.len(), 1);
        if matches!(fault, Fault::UploadAckLoss) {
            use std::os::unix::fs::PermissionsExt;
            let path = uploads[0].body["path"].as_str().unwrap();
            let metadata =
                fs::metadata(path).expect("unknown append must retain its prepared slot");
            assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        }
        fixture.assert_valid();
    }
}

#[tokio::test]
async fn malformed_streams_fail_closed_without_transport_replay() {
    for fault in [
        Fault::Eof,
        Fault::Gap,
        Fault::InvalidBase64,
        Fault::WrongProcess,
        Fault::MissingStarted,
        Fault::ReorderedStarted,
        Fault::ClosedOnly,
        Fault::WrongClosedReason,
        Fault::DuplicateClosed,
        Fault::OutputAfterExit,
        Fault::Resync,
        Fault::Oversized,
        Fault::Redirect,
    ] {
        let directory = temporary();
        let fixture = Fixture::new();
        let backend = open(&fixture, &directory.path().join("project")).await;
        let before = fixture.requests().len();
        fixture.fault(fault);
        assert!(backend.ensure_ready().await.is_err(), "accepted {fault:?}");
        let requests = fixture.requests();
        assert_eq!(
            requests[before..]
                .iter()
                .filter(|request| request.path.ends_with("/commands"))
                .count(),
            1,
            "replayed {fault:?}"
        );
        assert!(!requests
            .iter()
            .any(|request| request.path == "/credential-leak"));
        fixture.assert_valid();
    }
}

#[tokio::test]
async fn malicious_command_receipts_cannot_redirect_attach_credentials() {
    for fault in [
        Fault::AttachOrigin,
        Fault::AttachPath,
        Fault::WrongSandbox,
        Fault::WrongSession,
    ] {
        let directory = temporary();
        let fixture = Fixture::new();
        let backend = open(&fixture, &directory.path().join("project")).await;
        let before = fixture.requests().len();
        fixture.fault(fault);
        assert!(backend.ensure_ready().await.is_err(), "accepted {fault:?}");
        assert!(fixture.requests()[before..]
            .iter()
            .all(|request| !request.path.ends_with("/events")));
        fixture.assert_valid();
    }
}

#[tokio::test]
async fn reconnect_binds_configuration_and_refuses_missing_native_identity() {
    let directory = temporary();
    let fixture = Fixture::new();
    let backend = open(&fixture, &directory.path().join("project")).await;
    let descriptor = backend.descriptor();
    let before = fixture.requests().len();
    let other = SpaceProvider::new(
        SpaceConfig::new(
            &fixture.api_url,
            &fixture.data_url,
            SecretInput::new(MANAGEMENT.as_bytes().to_vec()),
            "another-user",
            env!("CARGO_BIN_EXE_workspace-supervisor"),
        )
        .unwrap(),
    );
    assert!(matches!(
        other.connect(&descriptor.reference).await,
        Err(Error::InvalidInput(_)) | Err(Error::Conflict(_))
    ));
    assert_eq!(fixture.requests().len(), before);
    fs::remove_dir_all(&descriptor.control_root).unwrap();
    assert!(provider(&fixture)
        .connect(&descriptor.reference)
        .await
        .is_err());
    assert!(
        !std::path::Path::new(&descriptor.control_root).exists(),
        "connect recreated deleted native identity"
    );
    fixture.assert_valid();
}

#[tokio::test]
async fn native_cancel_is_confirmed_and_rpc_deadline_does_not_restart_on_frames() {
    let directory = temporary();
    let fixture = Fixture::new();
    let backend = open(&fixture, &directory.path().join("project")).await;
    let run = backend
        .start(launch(RunId::generate(), &["sleep", "5"]))
        .await
        .unwrap();
    assert_eq!(backend.cancel(&run).await.unwrap(), RunState::Canceled);
    assert_eq!(backend.inspect_run(&run).await.unwrap(), RunState::Canceled);
    let short = SpaceProvider::new(
        SpaceConfig::new(
            &fixture.api_url,
            &fixture.data_url,
            SecretInput::new(MANAGEMENT.as_bytes().to_vec()),
            USER,
            env!("CARGO_BIN_EXE_workspace-supervisor"),
        )
        .unwrap()
        .with_rpc_timeout(Duration::from_millis(50))
        .unwrap(),
    );
    // connect itself sends Ready. Delayed stream must not be accepted even though
    // its underlying native query will eventually produce a complete response.
    let before = fixture.requests().len();
    fixture.fault(Fault::Delay);
    let started = std::time::Instant::now();
    assert!(short
        .connect(&backend.descriptor().reference)
        .await
        .is_err());
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(
        fixture.requests()[before..]
            .iter()
            .filter(|request| request.path.ends_with("/commands"))
            .count(),
        1
    );
    fixture.assert_valid();
}

#[tokio::test]
async fn timed_out_append_retains_private_slot_until_late_server_open() {
    use std::os::unix::fs::PermissionsExt;
    let directory = temporary();
    let fixture = Fixture::new();
    let original = open(&fixture, &directory.path().join("project")).await;
    let bounded = SpaceProvider::new(
        SpaceConfig::new(
            &fixture.api_url,
            &fixture.data_url,
            SecretInput::new(MANAGEMENT.as_bytes().to_vec()),
            USER,
            env!("CARGO_BIN_EXE_workspace-supervisor"),
        )
        .unwrap()
        .with_rpc_timeout(Duration::from_millis(200))
        .unwrap(),
    );
    let backend = bounded
        .connect(&original.descriptor().reference)
        .await
        .unwrap();
    fixture.fault(Fault::LateUpload);
    let mut request: ExecutionRequest = CommandSpec::new("/bin/sh")
        .args(["-c", "touch must-not-run"])
        .into();
    request
        .secrets
        .0
        .insert("PRIVATE_VALUE".into(), "private-late-append".into());
    assert!(backend.exec(request).await.is_err());
    let uploads: Vec<_> = fixture
        .requests()
        .into_iter()
        .filter(|request| request.path.ends_with("/files:write"))
        .collect();
    assert_eq!(uploads.len(), 1);
    let path = uploads[0].body["path"].as_str().unwrap();
    let before =
        fs::metadata(path).expect("unknown upload unlinked prepared slot before server open");
    assert_eq!(before.len(), 0);
    assert_eq!(before.permissions().mode() & 0o777, 0o600);
    tokio::time::sleep(Duration::from_millis(600)).await;
    let after = fs::metadata(path).unwrap();
    assert!(after.len() > 0);
    assert_eq!(after.permissions().mode() & 0o777, 0o600);
    assert!(!directory.path().join("project/must-not-run").exists());
    fixture.assert_valid();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_same_claim_starts_execute_one_native_workload() {
    let directory = temporary();
    let fixture = Fixture::new();
    let root = directory.path().join("project");
    fs::create_dir_all(&root).unwrap();
    let provider = SpaceProvider::new(
        SpaceConfig::new(
            &fixture.api_url,
            &fixture.data_url,
            SecretInput::new(MANAGEMENT.as_bytes().to_vec()),
            USER,
            env!("CARGO_BIN_EXE_workspace-supervisor"),
        )
        .unwrap()
        .with_rpc_timeout(Duration::from_secs(10))
        .unwrap(),
    );
    let backend = provider
        .open(
            OpenRequest::new(
                WorkspaceId::generate(),
                &SpaceOpenOptions::new(SANDBOX, root.to_str().unwrap()),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let id = RunId::generate();
    let barrier = Arc::new(tokio::sync::Barrier::new(8));
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let backend = backend.clone();
        let barrier = barrier.clone();
        let id = id.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            backend
                .start(launch(
                    id,
                    &[
                        "append",
                        "parallel-marker",
                        "once\n",
                        "0.1",
                        "0",
                        "parallel-done",
                    ],
                ))
                .await
                .unwrap()
        }));
    }
    let mut runs = Vec::new();
    for task in tasks {
        runs.push(task.await.unwrap());
    }
    for run in &runs {
        assert_eq!(run.id, id);
        assert_eq!(run.workspace, backend.descriptor().reference);
    }
    assert_eq!(
        terminal(backend.as_ref(), &runs[0]).await,
        RunState::Exited { exit_code: 0 }
    );
    assert_eq!(
        fs::read_to_string(root.join("parallel-marker")).unwrap(),
        "once\n"
    );
    assert_eq!(
        backend
            .read_output(&runs[0], OutputCursor::default(), 1024)
            .await
            .unwrap()
            .stdout,
        "parallel-done\n"
    );
    fixture.assert_valid();
}
