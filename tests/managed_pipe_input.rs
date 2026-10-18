//! Actual non-PTY native pipes, with modeled transport acknowledgement loss.
// The shared fixture's broader recovery helpers are used by other test binaries.
#[allow(dead_code)]
#[path = "support/recovery_continuation_fixture.rs"]
mod fixture;
mod support;
use fixture::*;
use std::{
    sync::{
        atomic::{AtomicU8, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use workspace_sdk::{
    infrastructure::FileStateRepository,
    providers::{ICommandRunner, LocalConfig, LocalProvider, NativeCommand, SystemCommandRunner},
    supervisor::protocol::{SupervisorOperation, SupervisorRequest},
    *,
};

async fn workspace(f: &Fixture) -> Workspace {
    f.sdk
        .open(
            &host("laptop"),
            OpenRequest {
                workspace_id: WorkspaceId::new("pipe-project").unwrap(),
                config: ProviderOptions::default(),
                resources: ResourceRequirements::default(),
            },
        )
        .await
        .unwrap()
}
fn request(id: &str, script: &str) -> LaunchRequest {
    LaunchRequest {
        id: RunId::new(id).unwrap(),
        execution: CommandSpec::new("/usr/bin/python3")
            .args(["-c", script])
            .timeout(30)
            .into(),
        output_limit_bytes: 512 * 1024,
    }
}
const ECHO: &str = "import os\nwhile True:\n b=os.read(0,8192)\n if not b: break\n os.write(1,b)\nos.write(2,b'EOF')";
async fn ready(run: &Run) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match run.state().await.unwrap() {
                RunState::Running => break,
                RunState::Starting => tokio::time::sleep(Duration::from_millis(5)).await,
                s => panic!("pipe not active: {s:?}"),
            }
        }
    })
    .await
    .unwrap();
}
async fn send_prefixes(run: &Run, bytes: &[u8]) {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut offset = 0;
        while offset < bytes.len() {
            let end = (offset + 65536).min(bytes.len());
            match run
                .write_input(&SecretInput::new(bytes[offset..end].to_vec()))
                .await
                .unwrap()
            {
                InputOutcome::Accepted { bytes: n } => {
                    assert!(n <= end - offset);
                    offset += n;
                    if n == 0 {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                }
                InputOutcome::Unknown => panic!("fixture cannot replay unknown input"),
            }
        }
    })
    .await
    .unwrap();
}
async fn output(run: &Run) -> (Vec<u8>, Vec<u8>) {
    let mut cursor = OutputCursor::default();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    for _ in 0..1000 {
        let p = run.output_bytes(cursor, 262144).await.unwrap();
        assert!(!p.gap);
        out.extend(p.stdout);
        err.extend(p.stderr);
        cursor = p.next;
        if p.complete {
            return (out, err);
        }
    }
    panic!("bounded output was not complete")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn piped_binary_prefixes_eof_reconnect_and_mode_identity() {
    run_case("native-pipe-roundtrip", |f| async move {
        let w = workspace(&f).await;
        // Keep the child alive after EOF so live-owner refusal is independent
        // of the conservative uncertainty allowed during control teardown.
        let gated_echo = format!("{ECHO}\nimport time,pathlib\ndeadline=time.monotonic()+15\nwhile not pathlib.Path('eof-release').exists():\n if time.monotonic()>deadline: raise SystemExit(91)\n time.sleep(0.005)");
        let launch = request("piped-binary", &gated_echo);
        let run = w.start_piped(launch.clone()).await.unwrap();
        ready(&run).await;
        let retry = w.start_piped(launch.clone()).await.unwrap();
        assert_eq!(run.reference(), retry.reference());
        assert!(matches!(
            w.start_with(launch).await,
            Err(Error::Conflict(_))
        ));
        let bytes = (0..131071).map(|n| (n % 256) as u8).collect::<Vec<_>>();
        send_prefixes(&run, &bytes).await;
        let reference = run.reference();
        drop(run);
        drop(retry);
        let reopened = f
            .sdk
            .connect(&reference.workspace)
            .await
            .unwrap()
            .run(reference.id);
        assert_eq!(
            reopened.close_input().await.unwrap(),
            InputCloseOutcome::Closed
        );
        assert_eq!(
            reopened.close_input().await.unwrap(),
            InputCloseOutcome::Closed
        );
        let refusal = reopened.write_input(&SecretInput::new(b"no-replay".to_vec())).await;
        assert!(matches!(refusal, Err(Error::Conflict(_))), "post-EOF observation: {refusal:?}");
        std::fs::write(std::path::Path::new(&w.descriptor().root).join("eof-release"), b"release").unwrap();
        assert_eq!(
            reopened.wait(Duration::from_secs(10)).await.unwrap(),
            RunState::Exited { exit_code: 0 }
        );
        assert_eq!(
            reopened.close_input().await.unwrap(),
            InputCloseOutcome::Closed
        );
        let (out, err) = output(&reopened).await;
        assert_eq!(out, bytes);
        assert_eq!(err, b"EOF");
        assert_eq!(f.run_count().await, 1);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn piped_admission_bounds_and_wrong_targets_refuse_before_effects() {
    run_case("native-pipe-refusal", |f| async move {
        let w = workspace(&f).await;
        for private in [false, true] {
            let mut launch = request("forbidden-initial", ECHO);
            if private {
                launch.execution.private_stdin =
                    Some(SecretInput::new(b"private-initial".to_vec()));
            } else {
                launch.execution.command.stdin = Some(b"ordinary-initial".to_vec());
            }
            assert!(matches!(
                w.start_piped(launch).await,
                Err(Error::InvalidInput(_))
            ));
            assert_eq!(f.run_count().await, 0);
        }
        let ordinary_request = request("ordinary", "print('ordinary')");
        let ordinary = w.start_with(ordinary_request.clone()).await.unwrap();
        // Golden legacy encoding: existing ordinary launch claims must retain
        // their salted tuple fingerprint and omit the new false mode field.
        use sha2::{Digest, Sha256};
        let control = w.descriptor().control_root;
        let mut hash = Sha256::new();
        hash.update(std::fs::read(format!("{control}/fingerprint-salt")).unwrap());
        hash.update(
            serde_json::to_vec(&(
                workspace_sdk::supervisor::protocol::SupervisorExecution {
                    command: ordinary_request.execution.command,
                    secrets: ordinary_request.execution.secrets,
                    private_stdin: ordinary_request.execution.private_stdin,
                    directory: workspace_sdk::supervisor::protocol::WorkingDirectory::Project(
                        WorkspacePath::root(),
                    ),
                },
                ordinary_request.output_limit_bytes,
                None::<TerminalSize>,
            ))
            .unwrap(),
        );
        let claim: serde_json::Value = serde_json::from_slice(
            &std::fs::read(format!("{control}/runs/ordinary/claim.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(claim["fingerprint"], format!("{:x}", hash.finalize()));
        assert!(claim.get("piped").is_none());
        assert!(ordinary
            .wait(Duration::from_secs(10))
            .await
            .unwrap()
            .is_terminal());
        assert!(matches!(
            w.start_piped(request("ordinary", "print('ordinary')"))
                .await,
            Err(Error::Conflict(_))
        ));
        assert!(matches!(
            ordinary.close_input().await,
            Err(Error::Unsupported(_))
        ));
        assert!(matches!(
            ordinary
                .write_input(&SecretInput::new(b"no".to_vec()))
                .await,
            Err(Error::Unsupported(_))
        ));
        let run = w.start_piped(request("input-bounds", ECHO)).await.unwrap();
        ready(&run).await;
        for bytes in [Vec::new(), vec![0; 65537]] {
            assert!(matches!(
                run.write_input(&SecretInput::new(bytes)).await,
                Err(Error::InvalidInput(_))
            ));
        }
        let other = f
            .sdk
            .open(
                &host("devbox"),
                OpenRequest {
                    workspace_id: WorkspaceId::new("wrong-pipe-project").unwrap(),
                    config: ProviderOptions::default(),
                    resources: ResourceRequirements::default(),
                },
            )
            .await
            .unwrap();
        assert!(matches!(
            other
                .backend()
                .write_input(
                    &run.reference(),
                    &SecretInput::new(b"wrong-target".to_vec())
                )
                .await,
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            other.backend().close_input(&run.reference()).await,
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(run.close_input().await.unwrap(), InputCloseOutcome::Closed);
        assert_eq!(
            run.wait(Duration::from_secs(10)).await.unwrap(),
            RunState::Exited { exit_code: 0 }
        );
        assert_eq!(output(&run).await, (Vec::new(), b"EOF".to_vec()));
        assert_eq!(f.run_count().await, 2);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pipe_backpressure_and_stalled_control_client_do_not_block_cancellation() {
    run_case("native-pipe-backpressure",|f|async move{
        let w=workspace(&f).await;let run=w.start_piped(request("full-pipe","import time; print('ready',flush=True); time.sleep(30)")).await.unwrap();ready(&run).await;
        let input=SecretInput::new(vec![73;65536]);let mut accepted=0;let mut blocked=false;
        tokio::time::timeout(Duration::from_secs(10),async{for _ in 0..64{match run.write_input(&input).await.unwrap(){InputOutcome::Accepted{bytes}=>{accepted+=bytes;if bytes==0{blocked=true;break}},InputOutcome::Unknown=>panic!("unknown backpressure fixture input")}}}).await.unwrap();
        assert!(accepted>0 && blocked,"actual pipe must fill without a queue");
        let control=w.descriptor().control_root;let script="import os,socket,sys,time; os.chdir(sys.argv[1]); s=socket.socket(socket.AF_UNIX); s.connect('stdin.sock'); s.sendall(b'{'); print('connected',flush=True); time.sleep(30)";
        let probe=w.start_with(LaunchRequest{id:RunId::new("stalled-control-client").unwrap(),execution:CommandSpec::new("/usr/bin/python3").args(["-c",script,&format!("{control}/runs/{}",run.reference().id)]).timeout(15).into(),output_limit_bytes:4096}).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10),async{loop{let p=probe.output(OutputCursor::default(),4096).await.unwrap();if p.stdout.contains("connected"){break}assert!(!p.complete,"probe did not connect: {}",p.stderr);tokio::time::sleep(Duration::from_millis(5)).await;}}).await.unwrap();
        let start=Instant::now();assert_eq!(tokio::time::timeout(Duration::from_secs(8),run.cancel()).await.unwrap().unwrap(),RunState::Canceled);assert!(start.elapsed()<Duration::from_secs(8));let cancel_ms=start.elapsed().as_secs_f64()*1000.0;
        assert_eq!(run.wait(Duration::from_secs(5)).await.unwrap(),RunState::Canceled);
        assert_eq!(probe.cancel().await.unwrap(),RunState::Canceled);
        assert_eq!(run.close_input().await.unwrap(),InputCloseOutcome::Closed);assert_eq!(f.run_count().await,2);
        println!("{}",serde_json::json!({"profile":"native-pipe-backpressure-cancel","cancel_ms":cancel_ms,"baseline":false}));
    }).await;
}

struct AckFault {
    mode: AtomicU8,
    inputs: AtomicUsize,
    closes: AtomicUsize,
}
#[async_trait::async_trait]
impl ICommandRunner for AckFault {
    async fn run(&self, command: NativeCommand) -> Result<ExecResult> {
        let op = serde_json::from_slice::<SupervisorRequest>(&command.stdin)
            .ok()
            .map(|r| r.operation);
        let input = matches!(op, Some(SupervisorOperation::PipeInput { .. }));
        let close = matches!(op, Some(SupervisorOperation::PipeCloseInput { .. }));
        if input {
            self.inputs.fetch_add(1, Ordering::SeqCst);
        }
        if close {
            self.closes.fetch_add(1, Ordering::SeqCst);
        }
        let result = SystemCommandRunner.run(command).await?;
        if input || close {
            let mode = self.mode.swap(0, Ordering::SeqCst);
            if mode == 1 {
                return Err(Error::State("injected lost pipe acknowledgement".into()));
            }
            if mode == 2 {
                return Ok(ExecResult{stdout:r#"{"status":"success","result":{"kind":"input","value":{"Accepted":{"bytes":999999}}}}"#.into(),..result});
            }
            if mode == 3 {
                return Ok(ExecResult {
                    stdout: "malformed acknowledgement".into(),
                    ..result
                });
            }
        }
        Ok(result)
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ambiguous_pipe_acknowledgements_never_replay_input_and_eof_retries_are_idempotent() {
    run_case("native-pipe-ackloss", |f| async move {
        let w = workspace(&f).await;
        let fault = Arc::new(AckFault {
            mode: AtomicU8::new(0),
            inputs: AtomicUsize::new(0),
            closes: AtomicUsize::new(0),
        });
        let mut config = LocalConfig::trusted_host(f.root.join("workers/laptop"));
        config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
        let sdk = WorkspaceSdk::builder(Arc::new(
            FileStateRepository::new(f.root.join("authority")).unwrap(),
        ))
        .provider(Arc::new(
            LocalProvider::with_runner(config, fault.clone())
                .unwrap()
                .with_id(host("laptop")),
        ))
        .unwrap()
        .build()
        .unwrap();
        let connected = sdk.connect(&w.reference()).await.unwrap();
        let run = connected
            .start_piped(request("ack-pipe", ECHO))
            .await
            .unwrap();
        ready(&run).await;
        for (mode, byte) in [(1, b'A'), (2, b'B'), (3, b'C')] {
            fault.mode.store(mode, Ordering::SeqCst);
            assert_eq!(
                run.write_input(&SecretInput::new(vec![byte]))
                    .await
                    .unwrap(),
                InputOutcome::Unknown
            );
        }
        fault.mode.store(1, Ordering::SeqCst);
        assert_eq!(run.close_input().await.unwrap(), InputCloseOutcome::Unknown);
        assert_eq!(
            run.wait(Duration::from_secs(10)).await.unwrap(),
            RunState::Exited { exit_code: 0 }
        );
        assert_eq!(run.close_input().await.unwrap(), InputCloseOutcome::Closed);
        // Even a definitive post-EOF refusal can lose its transport reply.
        // Preserve Unknown and prove this submission never becomes output.
        fault.mode.store(1, Ordering::SeqCst);
        assert_eq!(
            run.write_input(&SecretInput::new(b"AFTER-EOF".to_vec()))
                .await
                .unwrap(),
            InputOutcome::Unknown
        );
        assert_eq!(fault.inputs.load(Ordering::SeqCst), 4);
        assert_eq!(fault.closes.load(Ordering::SeqCst), 2);
        assert_eq!(output(&run).await, (b"ABC".to_vec(), b"EOF".to_vec()));
        assert_eq!(f.run_count().await, 1);
        let claim = std::fs::read_to_string(format!(
            "{}/runs/{}/claim.json",
            w.descriptor().control_root,
            run.reference().id
        ))
        .unwrap();
        assert!(!claim.contains("ABC"));
        assert!(claim.contains("piped"));
    })
    .await;
}
