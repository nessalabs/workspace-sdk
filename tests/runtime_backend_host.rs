#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use workspace_sdk::providers::{LocalConfig, LocalProvider};
use workspace_sdk::runtime::*;
use workspace_sdk::Error;
use workspace_sdk::{IBackend, IProvider, OpenRequest, OutputCursor, RunId, RunState, WorkspaceId};

mod support;

#[tokio::test]
async fn native_process_references_cannot_control_another_workspace() {
    let directory = tempfile::tempdir().unwrap();
    let left_backend = backend(&directory.path().join("left")).await;
    let right_backend = backend(&directory.path().join("right")).await;
    let left = BackendRuntimeHost::new(left_backend.clone());
    let right = BackendRuntimeHost::new(right_backend.clone());
    let first = left
        .start_interactive(
            "same-attempt",
            command(
                &left_backend.descriptor().root,
                support::workload_program(),
                &["sleep", "8"],
            ),
            false,
        )
        .await
        .unwrap();
    let second = right
        .start_interactive(
            "same-attempt",
            command(
                &right_backend.descriptor().root,
                support::workload_program(),
                &["sleep", "8"],
            ),
            false,
        )
        .await
        .unwrap();
    assert_eq!(first.id(), second.id());
    assert_ne!(first.host_scope(), second.host_scope());
    assert!(matches!(
        right.inspect_process(&first).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        right.cancel_process(&first).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        right
            .read_authentication_output(&first, OutputCursor::default(), 128)
            .await,
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(
        right.inspect_process(&second).await.unwrap(),
        RuntimeProcessState::Running
    );
    assert_eq!(
        left.cancel_process(&first).await.unwrap(),
        RuntimeProcessState::Exited { code: 130 }
    );
    assert_eq!(
        right.cancel_process(&second).await.unwrap(),
        RuntimeProcessState::Exited { code: 130 }
    );
    left_backend.destroy().await.unwrap();
    right_backend.destroy().await.unwrap();
}

async fn backend(root: &Path) -> Arc<dyn IBackend> {
    let mut config = LocalConfig::trusted_host(root);
    config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
    LocalProvider::new(config)
        .unwrap()
        .open(OpenRequest {
            workspace_id: WorkspaceId::generate(),
            config: Default::default(),
            resources: Default::default(),
        })
        .await
        .unwrap()
}

fn command(root: &str, program: &str, args: &[&str]) -> RuntimeCommand {
    RuntimeCommand {
        program: program.into(),
        args: args.iter().map(|arg| (*arg).into()).collect(),
        working_directory: root.into(),
        environment: BTreeMap::new(),
        secret_environment: BTreeMap::new(),
        remove_environment: Vec::new(),
        timeout_seconds: 10,
    }
}

#[tokio::test]
async fn local_adapter_checks_installed_runtime_launches_and_retains_no_input_secrets() {
    let directory = tempfile::tempdir().unwrap();
    let backend = backend(directory.path()).await;
    let descriptor = backend.descriptor();
    let host = Arc::new(BackendRuntimeHost::new(backend.clone()));
    let service = PreparationService::new(
        host.clone(),
        Arc::new(NoCredentials),
        Arc::new(MemoryPreparationStore::default()),
    );
    let recipe = generic("native-fixture", support::workload_program(), "1.0.0");
    let request = PreparationRequest {
        id: "prepare-native".into(),
        target_id: descriptor.reference.id.to_string(),
        owned_root: format!("{}/runtime", descriptor.control_root),
        working_directory: descriptor.root.clone(),
        recipe,
        environment: EnvironmentSpec::default(),
        authentication: Authentication::None,
    };
    let PreparationOutcome::Ready(runtime) = service.prepare(&request).await.unwrap() else {
        panic!("installed native runtime should be ready")
    };
    let mut launch = runtime.launch_command(["train"]).unwrap();
    // Fixture input is resolved into the transient secret map, never command env.
    launch
        .secret_environment
        .insert("FIXTURE_TOKEN".into(), SecretValue::new("private-fixture"));
    let execution = host.execution_request(launch.clone()).unwrap();
    assert!(!serde_json::to_string(&execution.command)
        .unwrap()
        .contains("private-fixture"));
    assert!(!format!("{execution:?}").contains("private-fixture"));
    let run = host.launch(RunId::generate(), launch, 8192).await.unwrap();
    timeout(Duration::from_secs(10), async {
        loop {
            match backend.inspect_run(&run).await.unwrap() {
                RunState::Starting | RunState::Running => sleep(Duration::from_millis(50)).await,
                state => {
                    assert_eq!(state, RunState::Exited { exit_code: 0 });
                    break;
                }
            }
        }
    })
    .await
    .unwrap();
    let output = backend
        .read_output(&run, OutputCursor::default(), 8192)
        .await
        .unwrap();
    assert!(output.stdout.contains("training completed"));
    for path in fs::read_dir(format!("{}/runs/{}", descriptor.control_root, run.id)).unwrap() {
        let path = path.unwrap().path();
        if path.is_file() {
            assert!(!String::from_utf8_lossy(&fs::read(path).unwrap()).contains("private-fixture"));
        }
    }
    let missing = host
        .exec(command(
            &descriptor.root,
            "workspace-sdk-certainly-missing-fixture",
            &["--version"],
        ))
        .await
        .unwrap();
    assert_eq!(missing.exit_code, 127);
    let stdin_result = host
        .exec_with_secret_stdin(
            command(
                &descriptor.root,
                support::workload_program(),
                &["native-stdin"],
            ),
            &SecretValue::new("stdin-fixture"),
        )
        .await
        .unwrap();
    assert_eq!(stdin_result.exit_code, 0);
    assert_eq!(stdin_result.stdout, "native input accepted\n");
}

#[tokio::test]
async fn private_credentials_are_mode_restricted_and_cannot_cross_symlinks() {
    let directory = tempfile::tempdir().unwrap();
    let backend = backend(directory.path()).await;
    let descriptor = backend.descriptor();
    let host = BackendRuntimeHost::new(backend);
    let owned = format!("{}/runtime/auth", descriptor.control_root);
    host.ensure_private_directory(&owned).await.unwrap();
    let private_output = host
        .exec(command(&owned, support::workload_program(), &["cwd"]))
        .await
        .unwrap();
    assert_eq!(private_output.exit_code, 0);
    assert_eq!(private_output.stdout.trim(), owned);

    let cache = format!("{owned}/auth.json");
    host.provision_private_file(&cache, &SecretValue::new("private-cache-fixture"))
        .await
        .unwrap();
    assert_eq!(
        fs::metadata(&owned).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&cache).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(fs::read_to_string(&cache).unwrap(), "private-cache-fixture");
    assert!(host
        .ensure_private_directory(&format!("{}/outside", descriptor.root))
        .await
        .is_err());
    assert!(host
        .ensure_private_directory(&format!("{}/../escape", descriptor.control_root))
        .await
        .is_err());
    let link = format!("{}/runtime/link", descriptor.control_root);
    symlink(&descriptor.root, &link).unwrap();
    assert!(host
        .ensure_private_directory(&format!("{link}/escape"))
        .await
        .is_err());
    assert!(!Path::new(&format!("{}/escape", descriptor.root)).exists());
    let file_link = format!("{owned}/linked.json");
    symlink(&cache, &file_link).unwrap();
    assert!(host
        .provision_private_file(&file_link, &SecretValue::new("replace-fixture"))
        .await
        .is_err());
    assert_eq!(fs::read_to_string(&cache).unwrap(), "private-cache-fixture");
}

#[tokio::test]
async fn native_device_interaction_is_a_reconnectable_managed_run() {
    let directory = tempfile::tempdir().unwrap();
    let backend = backend(directory.path()).await;
    let descriptor = backend.descriptor();
    let host = BackendRuntimeHost::new(backend.clone());
    // Synthetic native interaction: no actual sign-in or provider call.
    let login = command(&descriptor.root, support::workload_program(), &["login"]);
    let first = host
        .start_interactive("login-1", login.clone(), false)
        .await
        .unwrap();
    let second = host
        .start_interactive("login-1", login.clone(), false)
        .await
        .unwrap();
    assert_eq!(first, second);
    assert!(host
        .start_interactive("terminal-1", login, true)
        .await
        .is_err());
    timeout(Duration::from_secs(10), async {
        loop {
            match host.inspect_process(&first).await.unwrap() {
                RuntimeProcessState::Running => sleep(Duration::from_millis(50)).await,
                state => {
                    assert_eq!(state, RuntimeProcessState::Exited { code: 0 });
                    break;
                }
            }
        }
    })
    .await
    .unwrap();
    let reconnect = BackendRuntimeHost::new(backend);
    let output = reconnect
        .read_authentication_output(&first, OutputCursor::default(), 8192)
        .await
        .unwrap();
    assert_eq!(output.stdout.matches("use-device-code-fixture").count(), 1);
}

#[tokio::test]
async fn authentication_output_advances_across_split_unicode_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let backend = backend(directory.path()).await;
    let descriptor = backend.descriptor();
    let host = Arc::new(BackendRuntimeHost::new(backend.clone()));
    let store = Arc::new(MemoryPreparationStore::default());
    let service = PreparationService::new(host.clone(), Arc::new(NoCredentials), store.clone());
    let request = PreparationRequest {
        id: "unicode-output".into(),
        target_id: descriptor.reference.id.to_string(),
        owned_root: format!("{}/runtime", descriptor.control_root),
        working_directory: descriptor.root.clone(),
        recipe: generic("native-fixture", support::workload_program(), "1.0.0"),
        environment: EnvironmentSpec::default(),
        authentication: Authentication::None,
    };
    service.prepare(&request).await.unwrap();
    let process = host
        .start_interactive(
            "unicode-attempt",
            command(
                &descriptor.root,
                support::workload_program(),
                &["echo", "λ"],
            ),
            false,
        )
        .await
        .unwrap();
    // Bind a synthetic native attempt to the prepared receipt; no real sign-in.
    {
        let _lease = store.try_acquire(&request.id).await.unwrap();
        let mut receipt = store.load(&request.id).await.unwrap().unwrap();
        receipt.authentication_process = Some(process.clone());
        store.save(&receipt).await.unwrap();
    }
    timeout(Duration::from_secs(10), async {
        while host.inspect_process(&process).await.unwrap() == RuntimeProcessState::Running {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let mut cursor = OutputCursor::default();
    let mut text = String::new();
    for expected_offset in 1..=3 {
        let page = service
            .read_authentication_output(&request, &process, cursor, 2)
            .await
            .unwrap();
        assert_eq!(page.next.stdout, expected_offset);
        assert!(page.stdout.len() + page.stderr.len() <= 6);
        text.push_str(&page.stdout);
        cursor = page.next;
    }
    assert_eq!(text, "��\n");
    let end = service
        .read_authentication_output(&request, &process, cursor, 2)
        .await
        .unwrap();
    assert!(end.stdout.is_empty());
    backend.destroy().await.unwrap();
}
