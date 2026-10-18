//! Explicit child environment isolation through a real native supervisor.
// Shared recovery helpers are unused; its all-run ownership cleanup is reused.
#[allow(dead_code)]
#[path = "support/recovery_continuation_fixture.rs"]
mod fixture;
mod support;
use fixture::*;
use std::{
    fs,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use workspace_sdk::{
    infrastructure::FileStateRepository,
    providers::{ICommandRunner, LocalConfig, LocalProvider, NativeCommand, SystemCommandRunner},
    supervisor::protocol::{
        SupervisorExecution, SupervisorOperation, SupervisorRequest, WorkingDirectory,
    },
    *,
};

#[derive(Default)]
struct SyntheticHost {
    reject_clean: AtomicBool,
    make_relative: AtomicBool,
    clean_requests: AtomicUsize,
}
#[async_trait::async_trait]
impl ICommandRunner for SyntheticHost {
    async fn run(&self, mut command: NativeCommand) -> Result<ExecResult> {
        if let Ok(mut request) = serde_json::from_slice::<SupervisorRequest>(&command.stdin) {
            if let SupervisorOperation::StartPipedCleanEnvironment { execution, .. } =
                &mut request.operation
            {
                self.clean_requests.fetch_add(1, Ordering::SeqCst);
                assert!(!command
                    .args
                    .iter()
                    .any(|argument| argument.contains("synthetic-private")
                        || argument.contains("synthetic-secret")));
                if self.reject_clean.load(Ordering::SeqCst) {
                    return Err(Error::Unsupported(
                        "modeled old supervisor refuses new operation".into(),
                    ));
                }
                if self.make_relative.load(Ordering::SeqCst) {
                    // Exercise native admission AFTER public/backend checks.
                    execution.command.program = "python3".into();
                    command.stdin = serde_json::to_vec(&request).unwrap();
                }
            }
        }
        // Synthetic values only; no process-global mutation, shell, or credential
        // discovery. The real transport runner still owns timeout and child I/O.
        let mut args = vec![
            "SDK_HOST_SENTINEL=synthetic-host-value".into(),
            "PATH=/synthetic-sdk-search-path".into(),
            "HOME=/synthetic-sdk-home".into(),
            "REMOVE_ONLY=synthetic-remove".into(),
            "REINTRODUCED=synthetic-host".into(),
            "PRECEDENCE=synthetic-host".into(),
            command.program,
        ];
        args.extend(command.args);
        SystemCommandRunner
            .run(NativeCommand {
                program: "/usr/bin/env".into(),
                args,
                stdin: command.stdin,
                timeout: command.timeout,
            })
            .await
    }
}
async fn workspace(f: &Fixture, runner: Arc<SyntheticHost>) -> Workspace {
    let repository = Arc::new(FileStateRepository::new(f.root.join("authority")).unwrap());
    let mut config = LocalConfig::trusted_host(f.root.join("workers/laptop"));
    config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
    let sdk = WorkspaceSdk::builder(repository)
        .provider(Arc::new(
            LocalProvider::with_runner(config, runner)
                .unwrap()
                .with_id(host("laptop")),
        ))
        .unwrap()
        .build()
        .unwrap();
    sdk.open(
        &host("laptop"),
        OpenRequest {
            workspace_id: WorkspaceId::new("clean-environment").unwrap(),
            config: ProviderOptions::default(),
            resources: ResourceRequirements::default(),
        },
    )
    .await
    .unwrap()
}
const SCRIPT: &str = r#"import os,json
keys=['SDK_HOST_SENTINEL','PATH','HOME','REMOVE_ONLY','REINTRODUCED','PRECEDENCE','PUBLIC_ONLY','SECRET_ONLY']
os.write(1,(json.dumps({key:os.environ.get(key) for key in keys})+'\n').encode())
while True:
    data=os.read(0,8192)
    if not data: break
    os.write(1,data)
os.write(2,b'EOF')
"#;
fn request(id: &str) -> LaunchRequest {
    let mut command = CommandSpec::new("/usr/bin/python3")
        .args(["-c", SCRIPT])
        .timeout(30);
    command
        .env
        .insert("PUBLIC_ONLY".into(), "declared-public".into());
    command
        .env
        .insert("REINTRODUCED".into(), "public-after-remove".into());
    command
        .env
        .insert("PRECEDENCE".into(), "declared-public".into());
    command.env_remove = vec![
        "REMOVE_ONLY".into(),
        "REINTRODUCED".into(),
        "PRECEDENCE".into(),
    ];
    let mut execution: ExecutionRequest = command.into();
    // These deliberately printed values are synthetic fixtures, not credentials.
    execution
        .secrets
        .0
        .insert("PRECEDENCE".into(), "synthetic-secret-precedence".into());
    execution
        .secrets
        .0
        .insert("SECRET_ONLY".into(), "synthetic-private-only".into());
    LaunchRequest {
        id: RunId::new(id).unwrap(),
        execution,
        output_limit_bytes: 65536,
    }
}
async fn ready(run: &Run) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match run.state().await.unwrap() {
                RunState::Running => break,
                RunState::Starting => tokio::time::sleep(Duration::from_millis(5)).await,
                state => panic!("native pipe not ready: {state:?}"),
            }
        }
    })
    .await
    .unwrap();
}
async fn output(run: &Run) -> (Vec<u8>, Vec<u8>) {
    let mut cursor = OutputCursor::default();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    for _ in 0..16 {
        let page = run.output_bytes(cursor, 262144).await.unwrap();
        assert!(!page.gap);
        stdout.extend(page.stdout);
        stderr.extend(page.stderr);
        cursor = page.next;
        if page.complete {
            return (stdout, stderr);
        }
    }
    panic!("native output did not drain within fixture bound");
}
fn assert_explicit(environment: &serde_json::Value) {
    assert!(environment["REMOVE_ONLY"].is_null());
    assert_eq!(environment["REINTRODUCED"], "public-after-remove");
    assert_eq!(environment["PRECEDENCE"], "synthetic-secret-precedence");
    assert_eq!(environment["PUBLIC_ONLY"], "declared-public");
    assert_eq!(environment["SECRET_ONLY"], "synthetic-private-only");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn clean_pipe_excludes_host_environment_preserves_explicit_precedence_and_reconnects() {
    run_case("clean-managed-environment", |f| async move {
        let w = workspace(&f, Arc::new(SyntheticHost::default())).await;
        let inherited = w.start_piped(request("legacy-inherited")).await.unwrap();
        ready(&inherited).await;
        assert_eq!(
            inherited.close_input().await.unwrap(),
            InputCloseOutcome::Closed
        );
        assert_eq!(
            inherited.wait(Duration::from_secs(10)).await.unwrap(),
            RunState::Exited { exit_code: 0 }
        );
        let (legacy, legacy_error) = output(&inherited).await;
        assert_eq!(legacy_error, b"EOF");
        let legacy: serde_json::Value = serde_json::from_slice(&legacy).unwrap();
        assert_eq!(legacy["SDK_HOST_SENTINEL"], "synthetic-host-value");
        assert_eq!(legacy["PATH"], "/synthetic-sdk-search-path");
        assert_eq!(legacy["HOME"], "/synthetic-sdk-home");
        assert_explicit(&legacy);
        let launch = request("clean-pipe");
        let run = w
            .start_piped_with_clean_environment(launch.clone())
            .await
            .unwrap();
        ready(&run).await;
        assert_eq!(
            w.start_piped_with_clean_environment(launch.clone())
                .await
                .unwrap()
                .reference(),
            run.reference()
        );
        assert!(matches!(
            w.start_piped(launch.clone()).await,
            Err(Error::Conflict(_))
        ));
        assert!(matches!(
            w.start_with(launch).await,
            Err(Error::Conflict(_))
        ));
        let reference = run.reference();
        drop(run);
        let reconnected = f
            .sdk
            .connect(&reference.workspace)
            .await
            .unwrap()
            .run(reference.id);
        let bytes = [0, 255, 240, 159, 166, 128, 10];
        let mut offset = 0;
        tokio::time::timeout(Duration::from_secs(10), async {
            while offset < bytes.len() {
                match reconnected
                    .write_input(&SecretInput::new(bytes[offset..].to_vec()))
                    .await
                    .unwrap()
                {
                    InputOutcome::Accepted { bytes: count } => {
                        assert!(count <= bytes.len() - offset);
                        offset += count;
                        if count == 0 {
                            tokio::time::sleep(Duration::from_millis(5)).await;
                        }
                    }
                    InputOutcome::Unknown => panic!("cannot replay unknown native input"),
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(
            reconnected.close_input().await.unwrap(),
            InputCloseOutcome::Closed
        );
        assert_eq!(
            reconnected.wait(Duration::from_secs(10)).await.unwrap(),
            RunState::Exited { exit_code: 0 }
        );
        let (stdout, stderr) = output(&reconnected).await;
        assert_eq!(stderr, b"EOF");
        let separator = stdout.iter().position(|byte| *byte == b'\n').unwrap();
        let environment: serde_json::Value = serde_json::from_slice(&stdout[..separator]).unwrap();
        for key in ["SDK_HOST_SENTINEL", "PATH", "HOME"] {
            assert!(environment[key].is_null(), "inherited {key}");
        }
        assert_explicit(&environment);
        assert_eq!(&stdout[separator + 1..], bytes);
        let control = w.descriptor().control_root;
        let claim = fs::read(format!("{control}/runs/clean-pipe/claim.json")).unwrap();
        let claim = String::from_utf8(claim).unwrap();
        assert!(!claim.contains("synthetic-secret-precedence"));
        assert!(!claim.contains("synthetic-private-only"));
        assert!(!format!("{:?}", request("debug")).contains("synthetic-secret-precedence"));
        assert_eq!(f.run_count().await, 2);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn clean_pipe_rejects_initial_input_relative_program_and_existing_mode_rebinding() {
    run_case("clean-managed-mode-guards", |f| async move {
        let w = workspace(&f, Arc::new(SyntheticHost::default())).await;
        let mut invalid = request("invalid-relative");
        invalid.execution.command.program = "python3".into();
        assert!(matches!(
            w.start_piped_with_clean_environment(invalid).await,
            Err(Error::InvalidInput(_))
        ));
        for private in [false, true] {
            let mut invalid = request(if private {
                "invalid-private-input"
            } else {
                "invalid-input"
            });
            if private {
                invalid.execution.private_stdin = Some(SecretInput::new(b"never-sent".to_vec()));
            } else {
                invalid.execution.command.stdin = Some(b"never-sent".to_vec());
            }
            assert!(matches!(
                w.start_piped_with_clean_environment(invalid).await,
                Err(Error::InvalidInput(_))
            ));
        }
        assert_eq!(f.run_count().await, 0);
        let ordinary = w.start_with(request("ordinary-identity")).await.unwrap();
        assert_eq!(
            ordinary.wait(Duration::from_secs(10)).await.unwrap(),
            RunState::Exited { exit_code: 0 }
        );
        assert!(matches!(
            w.start_piped_with_clean_environment(request("ordinary-identity"))
                .await,
            Err(Error::Conflict(_))
        ));
        let piped = w.start_piped(request("piped-identity")).await.unwrap();
        ready(&piped).await;
        assert!(matches!(
            w.start_piped_with_clean_environment(request("piped-identity"))
                .await,
            Err(Error::Conflict(_))
        ));
        // Exact preexisting piped tuple encoding remains unchanged by clean mode.
        use sha2::{Digest, Sha256};
        let launch = request("piped-identity");
        let control = w.descriptor().control_root;
        let mut hash = Sha256::new();
        hash.update(fs::read(format!("{control}/fingerprint-salt")).unwrap());
        hash.update(b"workspace-sdk-piped-v1");
        let execution = SupervisorExecution {
            command: launch.execution.command,
            secrets: launch.execution.secrets,
            private_stdin: None,
            directory: WorkingDirectory::Project(WorkspacePath::root()),
        };
        hash.update(
            serde_json::to_vec(&(execution, launch.output_limit_bytes, None::<TerminalSize>))
                .unwrap(),
        );
        let claim: serde_json::Value = serde_json::from_slice(
            &fs::read(format!("{control}/runs/piped-identity/claim.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(claim["fingerprint"], format!("{:x}", hash.finalize()));
        assert_eq!(
            piped.close_input().await.unwrap(),
            InputCloseOutcome::Closed
        );
        assert_eq!(
            piped.wait(Duration::from_secs(10)).await.unwrap(),
            RunState::Exited { exit_code: 0 }
        );
        assert_eq!(f.run_count().await, 2);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unsupported_clean_mode_never_falls_back_and_native_rechecks_absolute_program() {
    run_case("clean-native-admission", |f| async move {
        let runner = Arc::new(SyntheticHost::default());
        let w = workspace(&f, runner.clone()).await;
        runner.reject_clean.store(true, Ordering::SeqCst);
        assert!(matches!(
            w.start_piped_with_clean_environment(request("unsupported-clean"))
                .await,
            Err(Error::Unsupported(_))
        ));
        assert_eq!(runner.clean_requests.load(Ordering::SeqCst), 1);
        assert_eq!(
            f.run_count().await,
            0,
            "unsupported mode must not use an ordinary or inherited-environment fallback"
        );
        runner.reject_clean.store(false, Ordering::SeqCst);
        runner.make_relative.store(true, Ordering::SeqCst);
        assert!(matches!(
            w.start_piped_with_clean_environment(request("native-relative"))
                .await,
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(runner.clean_requests.load(Ordering::SeqCst), 2);
        assert_eq!(
            f.run_count().await,
            0,
            "native relative-program refusal must precede durable claims"
        );
    })
    .await;
}
