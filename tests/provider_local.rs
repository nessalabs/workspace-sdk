mod support;
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    path::Path,
    sync::Arc,
    time::Duration,
};
use tempfile::TempDir;
use workspace_sdk::{
    providers::{LocalConfig, LocalOpenOptions, LocalProvider},
    *,
};

#[derive(serde::Deserialize)]
struct ProcessIdentity {
    supervisor_pid: i32,
    pid: i32,
}

fn provider(base: &TempDir) -> LocalProvider {
    let mut config = LocalConfig::trusted_host(base.path().join("owned"));
    config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
    LocalProvider::new(config).unwrap()
}
async fn workspace(base: &TempDir) -> Arc<dyn IBackend> {
    provider(base)
        .open(OpenRequest {
            workspace_id: WorkspaceId::generate(),
            config: ProviderOptions::default(),
            resources: ResourceRequirements::default(),
        })
        .await
        .unwrap()
}
fn launch(id: RunId, args: &[&str], limit: u64) -> LaunchRequest {
    LaunchRequest {
        id,
        execution: CommandSpec::new(support::workload_program())
            .args(args.iter().copied())
            .into(),
        output_limit_bytes: limit,
    }
}
async fn terminal(backend: &dyn IBackend, run: &RunRef) -> RunState {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let state = backend.inspect_run(run).await.unwrap();
            if !state.is_active() {
                return state;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn reconnect_survives_client_drop_and_prevents_duplicate_execution() {
    let base = tempfile::tempdir().unwrap();
    let backend = workspace(&base).await;
    let descriptor = backend.descriptor();
    let id = RunId::generate();
    let source = &["append", "launched", "once\n", "0.3", "0", "done"];
    let run = backend
        .start(launch(id.clone(), source, 4096))
        .await
        .unwrap();
    drop(backend);
    let backend = provider(&base)
        .connect(&descriptor.reference)
        .await
        .unwrap();
    backend
        .start(launch(id.clone(), source, 4096))
        .await
        .unwrap();
    assert!(matches!(
        terminal(backend.as_ref(), &run).await,
        RunState::Exited { exit_code: 0 }
    ));
    backend
        .start(launch(id.clone(), source, 4096))
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(format!("{}/launched", descriptor.root)).unwrap(),
        "once\n"
    );
    assert!(matches!(
        backend
            .start(launch(id, &["echo", "different"], 4096))
            .await,
        Err(Error::Conflict(_))
    ));
    let page = backend
        .read_output(&run, OutputCursor::default(), 1024)
        .await
        .unwrap();
    assert_eq!(page.stdout, "done\n");
    assert!(page.complete);
    assert!(!page.gap);
}
#[tokio::test]
async fn output_is_bounded_and_cursors_report_retention_gaps() {
    let base = tempfile::tempdir().unwrap();
    let backend = workspace(&base).await;
    let run = backend
        .start(launch(
            RunId::generate(),
            &["largeoutput", "20000", "tail", "err"],
            1024,
        ))
        .await
        .unwrap();
    assert_eq!(
        terminal(backend.as_ref(), &run).await,
        RunState::Exited { exit_code: 0 }
    );
    let mut cursor = OutputCursor::default();
    let mut stdout = String::new();
    let mut first = true;
    loop {
        let page = backend.read_output(&run, cursor, 128).await.unwrap();
        if first {
            assert!(page.gap);
            first = false;
        } else {
            assert!(!page.gap);
        }
        stdout.push_str(&page.stdout);
        cursor = page.next;
        if page.complete {
            break;
        }
    }
    assert!(stdout.ends_with("tail"));
    assert_eq!(stdout.len(), 512);
    assert_eq!(cursor.stdout, 20004);
    assert_eq!(cursor.stderr, 3);
    let log = format!(
        "{}/runs/{}/stdout.log",
        backend.descriptor().control_root,
        run.id
    );
    assert!(fs::metadata(log).unwrap().len() <= 520);
}
#[tokio::test]
async fn deadline_and_cancellation_stop_owned_process_groups() {
    let base = tempfile::tempdir().unwrap();
    let backend = workspace(&base).await;
    let mut request = launch(RunId::generate(), &["sleep", "30"], 4096);
    request.execution.command.timeout_seconds = Some(1);
    let run = backend.start(request).await.unwrap();
    assert_eq!(terminal(backend.as_ref(), &run).await, RunState::TimedOut);
    let run = backend
        .start(launch(
            RunId::generate(),
            &["spawn-descendant", "escaped", "2", "30"],
            4096,
        ))
        .await
        .unwrap();
    loop {
        if backend
            .read_output(&run, OutputCursor::default(), 1024)
            .await
            .unwrap()
            .stdout
            .contains("ready")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert_eq!(backend.cancel(&run).await.unwrap(), RunState::Canceled);
    tokio::time::sleep(Duration::from_millis(2200)).await;
    assert!(!Path::new(&backend.descriptor().root)
        .join("escaped")
        .exists());
}
#[tokio::test]
async fn file_chunks_preserve_binary_mode_and_reject_symlink_escapes() {
    let base = tempfile::tempdir().unwrap();
    let backend = workspace(&base).await;
    let path = WorkspacePath::new("nested/file").unwrap();
    backend
        .write_file(WriteFile {
            path: path.clone(),
            offset: 0,
            bytes: vec![0, 255, 3],
            truncate: true,
            executable: true,
        })
        .await
        .unwrap();
    backend
        .write_file(WriteFile {
            path: path.clone(),
            offset: 3,
            bytes: vec![4, 5],
            truncate: false,
            executable: true,
        })
        .await
        .unwrap();
    let chunk = backend
        .read_file(ReadFile {
            path: path.clone(),
            offset: 1,
            limit: 2,
        })
        .await
        .unwrap();
    assert_eq!(chunk.bytes, [255, 3]);
    assert_eq!(chunk.next_offset, 3);
    assert!(!chunk.eof);
    let chunk = backend
        .read_file(ReadFile {
            path,
            offset: 3,
            limit: 20,
        })
        .await
        .unwrap();
    assert_eq!(chunk.bytes, [4, 5]);
    assert!(chunk.eof);
    assert_eq!(
        fs::metadata(format!("{}/nested/file", backend.descriptor().root))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    symlink(
        base.path(),
        format!("{}/outside", backend.descriptor().root),
    )
    .unwrap();
    let escaped = WorkspacePath::new("outside/should-not-exist").unwrap();
    assert!(backend
        .write_file(WriteFile {
            path: escaped.clone(),
            offset: 0,
            bytes: vec![1],
            truncate: true,
            executable: false
        })
        .await
        .is_err());
    assert!(backend
        .read_file(ReadFile {
            path: escaped,
            offset: 0,
            limit: 1
        })
        .await
        .is_err());
    assert!(!base.path().join("should-not-exist").exists());
    let request = CommandSpec::new("true").cwd("outside").unwrap();
    assert!(backend.exec(request.into()).await.is_err());
}
#[tokio::test]
async fn ownership_blocks_attached_deletion_and_owned_delete_has_receipt() {
    let base = tempfile::tempdir().unwrap();
    let project = base.path().join("attached");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("precious"), "keep").unwrap();
    let backend = provider(&base)
        .open(OpenRequest {
            workspace_id: WorkspaceId::generate(),
            config: ProviderOptions::from_options(&LocalOpenOptions {
                attach: Some(project.clone()),
            })
            .unwrap(),
            resources: ResourceRequirements::default(),
        })
        .await
        .unwrap();
    assert!(matches!(
        backend.destroy().await,
        Err(Error::Unsupported(_))
    ));
    assert!(backend.stop().await.is_err());
    assert!(project.join("precious").exists());
    let owned = workspace(&base).await;
    let root = owned.descriptor().root;
    let deleted = owned.destroy().await.unwrap();
    assert_eq!(deleted.state, DeleteState::Completed);
    assert!(!Path::new(&root).exists());
    assert_eq!(
        owned
            .inspect_delete(&deleted.reference)
            .await
            .unwrap()
            .state,
        DeleteState::Completed
    );
    assert_eq!(
        owned.destroy().await.unwrap().reference.id,
        deleted.reference.id
    );
}
#[tokio::test]
async fn killed_supervisor_is_lost_and_same_id_never_relaunches() {
    let base = tempfile::tempdir().unwrap();
    let backend = workspace(&base).await;
    let id = RunId::generate();
    let source = &["append", "count", "one\n", "0", "10"];
    let run = backend
        .start(launch(id.clone(), source, 4096))
        .await
        .unwrap();
    let identity = Path::new(&backend.descriptor().control_root)
        .join("runs")
        .join(id.as_str())
        .join("process.json");
    let counter = Path::new(&backend.descriptor().root).join("count");
    tokio::time::timeout(Duration::from_secs(3), async {
        while !identity.exists() || !counter.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let values: ProcessIdentity = serde_json::from_slice(&fs::read(identity).unwrap()).unwrap();
    unsafe {
        libc::kill(values.supervisor_pid, libc::SIGKILL);
    }
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(matches!(
        backend.inspect_run(&run).await.unwrap(),
        RunState::Lost { .. }
    ));
    backend.start(launch(id, source, 4096)).await.unwrap();
    assert_eq!(
        fs::read_to_string(format!("{}/count", backend.descriptor().root)).unwrap(),
        "one\n"
    );
    let deletion = backend.destroy().await.unwrap();
    assert_eq!(deletion.state, DeleteState::Blocked);
    let reconnected = provider(&base)
        .connect(&backend.descriptor().reference)
        .await
        .unwrap();
    assert_eq!(
        reconnected
            .inspect_delete(&deletion.reference)
            .await
            .unwrap()
            .state,
        DeleteState::Blocked
    );
    // This fixture knows the still-live process identity. The library correctly
    // refuses to trust persisted PIDs when inspecting/canceling a lost run.
    unsafe {
        libc::killpg(values.pid, libc::SIGKILL);
    }
}
#[tokio::test]
async fn credentials_are_transient_and_missing_state_is_not_reinitialized() {
    let base = tempfile::tempdir().unwrap();
    let backend = workspace(&base).await;
    let mut request = launch(
        RunId::generate(),
        &["env-length", "SDK_TEST_KEY", "17"],
        4096,
    );
    request
        .execution
        .secrets
        .0
        .insert("SDK_TEST_KEY".into(), "do-not-persist-me".into());
    let run = backend.start(request).await.unwrap();
    assert_eq!(
        terminal(backend.as_ref(), &run).await,
        RunState::Exited { exit_code: 0 }
    );
    let dir = Path::new(&backend.descriptor().control_root)
        .join("runs")
        .join(run.id.as_str());
    for file in fs::read_dir(dir).unwrap() {
        let entry = file.unwrap();
        if entry.file_type().unwrap().is_file() {
            let bytes = fs::read(entry.path()).unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains("do-not-persist-me"));
        }
    }
    let reference = backend.descriptor().reference;
    fs::remove_dir_all(&backend.descriptor().control_root).unwrap();
    assert!(provider(&base)
        .connect(&reference)
        .await
        .unwrap()
        .ensure_ready()
        .await
        .is_err());
    assert!(!Path::new(&backend.descriptor().control_root).exists());
}
