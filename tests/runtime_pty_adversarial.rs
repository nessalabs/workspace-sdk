#![cfg(unix)]
//! Independent real-process and injected-transport failures at the public host boundary.
#[path = "support/pty_fixture.rs"]
mod fixture;

use async_trait::async_trait;
use fixture::{assert_no_secret_files, input, run_case, Fixture, Transcript, INPUT_SECRET};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::{symlink, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::time::{sleep, timeout};
use workspace_sdk::{
    providers::{ICommandRunner, LocalConfig, LocalProvider, NativeCommand, SystemCommandRunner},
    runtime::*,
    supervisor::protocol::{
        SupervisorFault, SupervisorFaultKind, SupervisorOperation, SupervisorRequest,
        SupervisorResponse, SupervisorValue,
    },
    DeleteState, Error, ExecResult, IProvider, OutputCursor, Result, SecretInput,
};

#[derive(Clone, Copy)]
enum ReplyFailure {
    Lost,
    NotFound,
    Unsupported,
    WrongVariant,
    OversizedCount,
}
struct LostInputAcknowledgement {
    calls: AtomicUsize,
    failure: ReplyFailure,
}

#[async_trait]
impl ICommandRunner for LostInputAcknowledgement {
    async fn run(&self, command: NativeCommand) -> Result<ExecResult> {
        let request: SupervisorRequest = serde_json::from_slice(&command.stdin)?;
        let input_bytes = match &request.operation {
            SupervisorOperation::TerminalInput {
                input: Some(input), ..
            } => Some(input.expose().len()),
            _ => None,
        };
        let mut result = SystemCommandRunner.run(command).await?;
        if let Some(bytes) = input_bytes {
            self.calls.fetch_add(1, Ordering::SeqCst);
            // Delivery happened through the real supervisor. Lose only its reply.
            let replacement = match self.failure {
                ReplyFailure::Lost => {
                    return Err(Error::Timeout("injected lost input acknowledgement".into()))
                }
                ReplyFailure::NotFound => {
                    return Err(Error::NotFound(
                        "injected after actual input delivery".into(),
                    ))
                }
                ReplyFailure::Unsupported => {
                    return Err(Error::Unsupported(
                        "injected after actual input delivery".into(),
                    ))
                }
                ReplyFailure::WrongVariant => SupervisorValue::Empty,
                ReplyFailure::OversizedCount => {
                    SupervisorValue::Input(RuntimeInputOutcome::Accepted { bytes: bytes + 1 })
                }
            };
            result.stdout = serde_json::to_string(&SupervisorResponse::Success(replacement))?;
        }
        Ok(result)
    }
}

#[tokio::test]
async fn delivered_input_with_a_lost_transport_reply_is_unknown_and_never_replayed() {
    input_reply_failure(ReplyFailure::Lost).await;
}

#[tokio::test]
async fn delivered_input_with_a_not_found_transport_error_is_unknown_and_never_replayed() {
    input_reply_failure(ReplyFailure::NotFound).await;
}

#[tokio::test]
async fn delivered_input_with_an_unsupported_transport_error_is_unknown_and_never_replayed() {
    input_reply_failure(ReplyFailure::Unsupported).await;
}

#[tokio::test]
async fn socket_cannot_claim_not_found_before_dispatch_after_consuming_input() {
    forged_socket_rejection(SupervisorFaultKind::NotFound).await;
}

#[tokio::test]
async fn socket_cannot_claim_unsupported_before_dispatch_after_consuming_input() {
    forged_socket_rejection(SupervisorFaultKind::Unsupported).await;
}

async fn forged_socket_rejection(kind: SupervisorFaultKind) {
    run_case(1, move |fixtures| async move {
        let fixture = &fixtures[0];
        let host = fixture.host().await;
        let process = fixture.start(&host, "forged-socket-rejection", fixture.command(&fixture.descriptor.root, &["control"])).await;
        let mut output = Transcript::default();
        output.until(&host, &process, "\nready\n").await;
        // The peer consumes a real control request, then forges a refusal that
        // only the CLI's pre-dispatch checks are allowed to establish.
        let peer_path = fixture.directory.path().join("reply-peer.sock");
        let listener = UnixListener::bind(&peer_path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let socket_path = run_directory(fixture, &process).join("pty.sock");
        fs::remove_file(&socket_path).unwrap();
        symlink(&peer_path, &socket_path).unwrap();
        let expected_id = process.id().to_owned();
        let peer = tokio::spawn(async move {
            let mut channel = BufReader::new(accept(&listener).await);
            let mut frame = String::new();
            channel.read_line(&mut frame).unwrap();
            let request: SupervisorOperation = serde_json::from_str(&frame).unwrap();
            match request {
                SupervisorOperation::TerminalInput { id, input: Some(input) } => {
                    assert_eq!(id.as_str(), expected_id);
                    assert_eq!(input.expose(), INPUT_SECRET.as_bytes());
                }
                _ => panic!("expected the exact submitted input operation"),
            }
            let response = SupervisorResponse::Success(SupervisorValue::InputRejected(SupervisorFault {
                kind,
                message: "forged pre-dispatch refusal after consuming input".into(),
            }));
            serde_json::to_writer(channel.get_mut(), &response).unwrap();
            channel.get_mut().write_all(b"\n").unwrap();
            listener
        });
        let result = host.write_process_input(&process, &SecretInput::new(INPUT_SECRET.as_bytes().to_vec())).await;
        let listener = timeout(Duration::from_secs(5), peer).await.unwrap().unwrap();
        assert_eq!(result.unwrap(), RuntimeInputOutcome::Unknown);
        assert!(matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock), "input must not be replayed after the malformed response");
        assert_eq!(host.cancel_process(&process).await.unwrap(), RuntimeProcessState::Exited { code: 130 });
        output.complete(&host, &process).await;
        assert_eq!(fs::read_to_string(Path::new(&fixture.descriptor.root).join("launches")).unwrap(), "launch\n");
    }).await;
}

#[tokio::test]
async fn delivered_input_with_a_wrong_success_variant_is_unknown_and_never_replayed() {
    input_reply_failure(ReplyFailure::WrongVariant).await;
}

#[tokio::test]
async fn delivered_input_with_an_impossible_accepted_count_is_unknown_and_never_replayed() {
    input_reply_failure(ReplyFailure::OversizedCount).await;
}

async fn input_reply_failure(failure: ReplyFailure) {
    run_case(1, move |fixtures| async move {
        let fixture = &fixtures[0];
        let host = fixture.host().await;
        let process = fixture
            .start(
                &host,
                "lost-input-reply",
                fixture.command(&fixture.descriptor.root, &["control"]),
            )
            .await;
        let mut output = Transcript::default();
        output.until(&host, &process, "\nready\n").await;

        let runner = Arc::new(LostInputAcknowledgement {
            calls: AtomicUsize::new(0),
            failure,
        });
        let mut config = LocalConfig::trusted_host(fixture.directory.path().join("workspaces"));
        config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
        let provider = LocalProvider::with_runner(config, runner.clone()).unwrap();
        let uncertain = BackendRuntimeHost::new(
            provider
                .connect(&fixture.descriptor.reference)
                .await
                .unwrap(),
        );
        assert_eq!(
            uncertain
                .write_process_input(
                    &process,
                    &SecretInput::new(INPUT_SECRET.as_bytes().to_vec())
                )
                .await
                .unwrap(),
            RuntimeInputOutcome::Unknown
        );
        output.until(&host, &process, "input:accepted\n").await;
        input(&host, &process, b"quit\n").await;
        output.complete(&host, &process).await;
        assert_eq!(runner.calls.load(Ordering::SeqCst), 1);
        assert_eq!(output.text.matches("input:accepted\n").count(), 1);
        assert_eq!(
            fs::read_to_string(Path::new(&fixture.descriptor.root).join("launches")).unwrap(),
            "launch\n"
        );
        assert_no_secret_files(Path::new(&fixture.descriptor.control_root));
    })
    .await;
}

fn run_directory(fixture: &Fixture, process: &RuntimeProcessRef) -> PathBuf {
    Path::new(&fixture.descriptor.control_root)
        .join("runs")
        .join(process.id())
}

async fn confirmed_exit(host: &dyn IRuntimeHost, process: &RuntimeProcessRef, code: i32) {
    timeout(Duration::from_secs(5), async {
        loop {
            match host.inspect_process(process).await.unwrap() {
                RuntimeProcessState::Running => sleep(Duration::from_millis(10)).await,
                state => {
                    assert_eq!(state, RuntimeProcessState::Exited { code });
                    break;
                }
            }
        }
    })
    .await
    .expect("known terminal outcome must become observable");
}

#[tokio::test]
async fn missing_terminal_program_exits_127_with_retained_diagnostic_and_safe_lifecycle() {
    run_case(1, |fixtures| async move {
        let fixture = &fixtures[0];
        let host = fixture.host().await;
        let search = Path::new(&fixture.descriptor.root).join("empty-path");
        fs::create_dir(&search).unwrap();
        let mut command = fixture.command(&fixture.descriptor.root, &[]);
        command.program = "missing-terminal-program".into();
        command
            .environment
            .insert("PATH".into(), search.to_str().unwrap().into());
        let process = fixture
            .start(&host, "missing-program", command.clone())
            .await;
        confirmed_exit(&host, &process, 127).await;
        let page = host
            .read_process_output(&process, OutputCursor::default(), 4096)
            .await
            .unwrap();
        assert!(page.complete && !page.gap);
        let diagnostic = format!("{}{}", page.stdout, page.stderr);
        assert!(
            !diagnostic.trim().is_empty(),
            "known launch rejection requires a diagnostic"
        );
        assert!(
            diagnostic.len() <= 1024,
            "launch diagnostics must be bounded"
        );
        let directory = run_directory(fixture, &process);
        let claim = fs::read(directory.join("claim.json")).unwrap();
        assert!(
            !directory.join("process.json").exists(),
            "no child was admitted"
        );
        let reconnected = fixture.host().await;
        assert_eq!(
            reconnected
                .start_interactive("missing-program", command, true)
                .await
                .unwrap(),
            process
        );
        confirmed_exit(&reconnected, &process, 127).await;
        let repeated = reconnected
            .read_process_output(&process, OutputCursor::default(), 4096)
            .await
            .unwrap();
        assert_eq!(repeated.stdout, page.stdout);
        assert_eq!(repeated.stderr, page.stderr);
        assert_eq!(repeated.next.stdout, page.next.stdout);
        assert_eq!(repeated.next.stderr, page.next.stderr);
        assert!(repeated.complete && !repeated.gap);
        assert_eq!(fs::read(directory.join("claim.json")).unwrap(), claim);
        reconnected.backend().stop().await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn nonexistent_terminal_run_returns_not_found_without_delivering_input() {
    run_case(1, |fixtures| async move {
        let fixture = &fixtures[0];
        let host = fixture.host().await;
        let process = fixture
            .start(
                &host,
                "existing-input-owner",
                fixture.command(&fixture.descriptor.root, &["control"]),
            )
            .await;
        let mut output = Transcript::default();
        output.until(&host, &process, "\nready\n").await;
        let missing = RuntimeProcessRef::new(process.host_scope(), "missing-terminal-run").unwrap();
        assert!(!run_directory(fixture, &missing).exists());
        let result = host
            .write_process_input(
                &missing,
                &SecretInput::new(INPUT_SECRET.as_bytes().to_vec()),
            )
            .await;
        assert!(
            matches!(result, Err(Error::NotFound(_))),
            "a missing run is a definite pre-delivery rejection: {result:?}"
        );
        assert!(!run_directory(fixture, &missing).exists());
        input(&host, &process, b"quit\n").await;
        output.complete(&host, &process).await;
        assert!(
            !output.text.contains("input:accepted"),
            "input must not be rebound to another live run"
        );
        assert_eq!(
            fs::read_to_string(Path::new(&fixture.descriptor.root).join("launches")).unwrap(),
            "launch\n"
        );
        assert_no_secret_files(Path::new(&fixture.descriptor.control_root));
    })
    .await;
}

#[tokio::test]
async fn absent_terminal_socket_does_not_replace_confirmed_exit_with_lost() {
    socket_cleanup_failure(false).await;
}

#[tokio::test]
async fn terminal_socket_unlink_failure_does_not_replace_confirmed_exit_with_lost() {
    socket_cleanup_failure(true).await;
}

async fn socket_cleanup_failure(replace_with_directory: bool) {
    run_case(1, move |fixtures| async move {
        let fixture = &fixtures[0];
        let host = fixture.host().await;
        let mut command = fixture.command(&fixture.descriptor.root, &[]);
        command.program = "/bin/sh".into();
        command.args = vec!["-c".into(), "printf socket-ready; while [ ! -f release-exit ]; do sleep 0.02; done; printf final-output; exit 7".into()];
        let process = fixture.start(&host, "socket-cleanup", command).await;
        let mut output = Transcript::default();
        output.until(&host, &process, "socket-ready").await;
        let socket = run_directory(fixture, &process).join("pty.sock");
        fs::remove_file(&socket).unwrap();
        if replace_with_directory {
            fs::create_dir(&socket).unwrap();
        }
        fs::write(Path::new(&fixture.descriptor.root).join("release-exit"), b"release").unwrap();
        confirmed_exit(&host, &process, 7).await;
        output.complete(&host, &process).await;
        assert_eq!(output.text, "socket-readyfinal-output");
        let reconnected = fixture.host().await;
        confirmed_exit(&reconnected, &process, 7).await;
        let settled = reconnected.read_process_output(&process, output.cursor.clone(), 4096).await.unwrap();
        assert!(settled.complete && !settled.gap && settled.stdout.is_empty() && settled.stderr.is_empty());
        assert_eq!(settled.next.stdout, output.cursor.stdout);
        assert_eq!(settled.next.stderr, output.cursor.stderr);
        if replace_with_directory {
            fs::remove_dir(&socket).unwrap();
        }
        reconnected.backend().stop().await.unwrap();
    }).await;
}

async fn accept(listener: &UnixListener) -> UnixStream {
    timeout(Duration::from_secs(5), async {
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    return stream;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    sleep(Duration::from_millis(10)).await
                }
                Err(error) => panic!("fixture accept failed: {error}"),
            }
        }
    })
    .await
    .expect("fixture control connection deadline")
}

struct OwnedClient(Child);
impl Drop for OwnedClient {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

async fn slow_client(
    fixture: &Fixture,
    process: &RuntimeProcessRef,
    name: &str,
) -> (OwnedClient, BufReader<UnixStream>) {
    let socket = fixture.directory.path().join(name);
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let helper = fixture.command(&fixture.descriptor.root, &[]).program;
    let child = OwnedClient(
        Command::new(helper)
            .arg("trickle-client")
            .arg(run_directory(fixture, process))
            .arg(&socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut control = BufReader::new(accept(&listener).await);
    let mut ready = String::new();
    control.read_line(&mut ready).unwrap();
    assert_eq!(ready, "connected\n");
    (child, control)
}

#[tokio::test]
async fn trickled_socket_frames_have_a_total_deadline_and_cannot_starve_cancellation() {
    run_case(1, |fixtures| async move {
        let fixture = &fixtures[0];
        let host = fixture.host().await;
        let process = fixture
            .start(
                &host,
                "slow-client",
                fixture.command(&fixture.descriptor.root, &["control"]),
            )
            .await;
        let mut output = Transcript::default();
        output.until(&host, &process, "\nready\n").await;
        let (_client, mut control) = slow_client(fixture, &process, "trickle-a.sock").await;
        let mut closed = String::new();
        control.read_line(&mut closed).unwrap();
        let elapsed: u64 = closed
            .trim()
            .strip_prefix("closed:")
            .expect("owner must close the incomplete frame")
            .parse()
            .unwrap();
        assert!(
            elapsed < 1200,
            "a continuously trickled frame must have a total deadline: {elapsed} ms"
        );
        assert_eq!(
            host.inspect_process(&process).await.unwrap(),
            RuntimeProcessState::Running
        );
        input(&host, &process, b"size\n").await;
        output.until(&host, &process, "size:80x24\n").await;

        let (_second, _control) = slow_client(fixture, &process, "trickle-b.sock").await;
        let started = Instant::now();
        assert_eq!(
            timeout(Duration::from_secs(3), host.cancel_process(&process))
                .await
                .unwrap()
                .unwrap(),
            RuntimeProcessState::Exited { code: 130 }
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "slow control must not monopolize the owner cancellation loop"
        );
        output.complete(&host, &process).await;
    })
    .await;
}

#[tokio::test]
async fn pipe_runs_do_not_gain_terminal_controls_or_change_modes_under_the_same_identity() {
    run_case(1, |fixtures| async move {
        let fixture = &fixtures[0];
        let host = fixture.host().await;
        let mut command = fixture.command(&fixture.descriptor.root, &[]);
        command.program = "/bin/echo".into();
        command.args = vec!["pipe-fixture".into()];
        let process = host
            .start_interactive("pipe-only", command.clone(), false)
            .await
            .unwrap();
        timeout(Duration::from_secs(5), async {
            while host.inspect_process(&process).await.unwrap() == RuntimeProcessState::Running {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            host.inspect_process(&process).await.unwrap(),
            RuntimeProcessState::Exited { code: 0 }
        );
        assert!(matches!(
            host.write_process_input(&process, &SecretInput::new(b"ignored".to_vec()))
                .await,
            Err(Error::Unsupported(_))
        ));
        assert!(matches!(
            host.resize_process(&process, RuntimeTerminalSize::new(100, 30).unwrap())
                .await,
            Err(Error::Unsupported(_))
        ));
        assert!(matches!(
            host.start_interactive("pipe-only", command, true).await,
            Err(Error::Conflict(_))
        ));
        let page = host
            .read_process_output(&process, OutputCursor::default(), 128)
            .await
            .unwrap();
        assert_eq!(page.stdout, "pipe-fixture\n");
        assert!(page.complete && !page.gap);
    })
    .await;
}

#[tokio::test]
async fn persisted_incomplete_drain_is_visible_after_reconnect_without_inventing_output() {
    run_case(1, |fixtures| async move {
        let fixture = &fixtures[0];
        let host = fixture.host().await;
        let process = fixture
            .start(
                &host,
                "incomplete-drain",
                fixture.command(&fixture.descriptor.root, &["control"]),
            )
            .await;
        let mut output = Transcript::default();
        output.until(&host, &process, "\nready\n").await;
        input(&host, &process, b"quit\n").await;
        output.complete(&host, &process).await;
        // Inject the exact durable marker emitted by an incomplete native drain.
        // This verifies the persistence-to-public-output boundary, not native EOF generation.
        fs::write(
            run_directory(fixture, &process).join("output-gap"),
            b"incomplete drain",
        )
        .unwrap();
        drop(host);
        let rebound = fixture.host().await;
        let page = rebound
            .read_process_output(&process, output.cursor.clone(), 128)
            .await
            .unwrap();
        assert!(
            page.gap,
            "durable incomplete-drain evidence must survive public output reads"
        );
        assert!(page.complete && page.stdout.is_empty() && page.stderr.is_empty());
        assert_eq!(page.next.stdout, output.cursor.stdout);
        assert_eq!(page.next.stderr, output.cursor.stderr);
    })
    .await;
}

#[tokio::test]
async fn terminal_path_search_skips_a_non_executable_shadow_before_a_valid_program() {
    run_case(1, |fixtures| async move {
        let fixture = &fixtures[0];
        let root = Path::new(&fixture.descriptor.root);
        let first = root.join("path-shadow");
        let denied = root.join("path-owner-denied");
        let second = root.join("path-executable");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&denied).unwrap();
        fs::create_dir(&second).unwrap();
        fs::write(first.join("pty-fixture"), b"not executable\n").unwrap();
        fs::set_permissions(first.join("pty-fixture"), fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(denied.join("pty-fixture"), b"owner cannot execute\n").unwrap();
        fs::set_permissions(
            denied.join("pty-fixture"),
            fs::Permissions::from_mode(0o010),
        )
        .unwrap();
        let mut command = fixture.command(&fixture.descriptor.root, &["control"]);
        symlink(&command.program, second.join("pty-fixture")).unwrap();
        command.program = "pty-fixture".into();
        command.environment.insert(
            "PATH".into(),
            format!(
                "{}:{}:{}",
                first.display(),
                denied.display(),
                second.display()
            ),
        );
        let host = fixture.host().await;
        let process = fixture.start(&host, "path-search", command).await;
        timeout(Duration::from_secs(5), async {
            while !run_directory(fixture, &process)
                .join("process.json")
                .exists()
            {
                assert_ne!(
                    host.inspect_process(&process).await.unwrap(),
                    RuntimeProcessState::Lost,
                    "a non-executable PATH shadow must not hide the later executable"
                );
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let mut output = Transcript::default();
        output.until(&host, &process, "\nready\n").await;
        input(&host, &process, b"quit\n").await;
        output.complete(&host, &process).await;
        assert_eq!(
            host.inspect_process(&process).await.unwrap(),
            RuntimeProcessState::Exited { code: 0 }
        );
    })
    .await;
}

fn running(pid: i32) -> bool {
    let result = Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "stat="])
        .output()
        .unwrap();
    assert!(result.status.success() || result.status.code() == Some(1));
    assert!(result.stderr.is_empty());
    let state = String::from_utf8(result.stdout).unwrap();
    !state.trim().is_empty() && !state.trim().starts_with('Z')
}

#[tokio::test]
async fn killed_terminal_owner_becomes_lost_releases_its_lock_and_never_relaunches_the_claim() {
    // Lost intentionally forbids ordinary destroy. Keep the fixture outside the
    // case task, retain independent workload cleanup channels through checks,
    // then remove only our TempDir after proving the workload tree has stopped.
    let fixture = Arc::new(Fixture::new().await);
    let observed = fixture.clone();
    let mut case = tokio::spawn(async move {
        let socket = observed.directory.path().join("owner-tree.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let host = observed.host().await;
        let command = observed.command(
            &observed.descriptor.root,
            &["owner-tree", socket.to_str().unwrap()],
        );
        let process = observed.start(&host, "lost-owner", command.clone()).await;
        let mut channels = Vec::new();
        let mut pids = Vec::new();
        for _ in 0..2 {
            let mut channel = BufReader::new(accept(&listener).await);
            let mut line = String::new();
            channel.read_line(&mut line).unwrap();
            pids.push(line.trim().parse::<i32>().unwrap());
            channels.push(channel);
        }
        let directory = run_directory(&observed, &process);
        timeout(Duration::from_secs(5), async {
            while !directory.join("process.json").exists() {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("owner must persist its process identity after admission");
        let identity: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("process.json")).unwrap()).unwrap();
        let owner = identity["supervisor_pid"].as_i64().unwrap() as i32;
        let leader = identity["pid"].as_i64().unwrap() as i32;
        assert!(pids.contains(&leader));
        assert!(running(owner) && pids.iter().all(|pid| running(*pid)));
        assert_eq!(
            host.inspect_process(&process).await.unwrap(),
            RuntimeProcessState::Running
        );
        // SAFETY: this is the positively observed owner of our isolated, live
        // fixture, obtained from its private record before its bounded lifetime ends.
        assert_eq!(unsafe { libc::kill(owner, libc::SIGKILL) }, 0);
        timeout(Duration::from_secs(5), async {
            while host.inspect_process(&process).await.unwrap() != RuntimeProcessState::Lost {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("inherited descriptors must not keep a dead owner's active lock alive");
        timeout(Duration::from_secs(5), async {
            while pids.iter().any(|pid| running(*pid)) {
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("owner-death backstop must stop both leader and descendant");
        assert_eq!(
            host.start_interactive("lost-owner", command.clone(), true)
                .await
                .unwrap(),
            process
        );
        assert_eq!(
            host.inspect_process(&process).await.unwrap(),
            RuntimeProcessState::Lost
        );
        assert_eq!(
            fs::read_to_string(Path::new(&observed.descriptor.root).join("launches")).unwrap(),
            "launch\n"
        );
        assert!(matches!(
            host.start_interactive("lost-owner", command, false).await,
            Err(Error::Conflict(_))
        ));
        assert_eq!(
            host.backend().destroy().await.unwrap().state,
            DeleteState::Blocked
        );
        assert!(
            directory.join("claim.json").exists(),
            "uncertain claim must remain durable"
        );
        drop(channels);
    });
    let outcome = match timeout(Duration::from_secs(20), &mut case).await {
        Ok(result) => result,
        Err(_) => {
            case.abort();
            case.await
        }
    };
    // If the case failed before SIGKILL, cancellation still cleans the live run.
    // When it reached Lost, refusing lifecycle mutation is the asserted behavior.
    let host = fixture.host().await;
    let _ = timeout(Duration::from_secs(6), host.backend().stop()).await;
    outcome.expect("owner-death case failed");
}
