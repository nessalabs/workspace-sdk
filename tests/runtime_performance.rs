#![cfg(unix)]

//! Opt-in qualification of real local managed runs; latency samples are reports,
//! while retention, replay, idempotency, and cancellation are pass/fail contracts.
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::time::{sleep, timeout};
use workspace_sdk::providers::{LocalConfig, LocalProvider};
use workspace_sdk::{
    CommandSpec, IBackend, IProvider, LaunchRequest, OpenRequest, OutputCursor, OutputPage, RunId,
    RunRef, RunState, WorkspaceId,
};

const RETENTION_BYTES: u64 = 32 * 1024;
const PAGE_BYTES: usize = 4096;
const LOAD_BYTES: u64 = 128 * 1024;
const SAMPLES: usize = 3;
const WATCHDOG: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

// A native producer keeps both output pipes active during
// reconnect and cancellation. The marker detects accidental duplicate launches.
const PRODUCER: &str = r#"
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::thread;
use std::time::{Duration, Instant};
fn main() {
    let mut marker = OpenOptions::new().create(true).append(true).open("launches").unwrap();
    marker.write_all(b"once\n").unwrap();
    marker.sync_all().unwrap();
    let started = Instant::now();
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    while started.elapsed() < Duration::from_secs(20) {
        stdout.write_all(&[b'x'; 4096]).unwrap();
        stdout.flush().unwrap();
        stderr.write_all(&[b'e'; 4096]).unwrap();
        stderr.flush().unwrap();
        thread::sleep(Duration::from_millis(1));
    }
    std::process::exit(1);
}
"#;

#[derive(Serialize)]
struct Sample {
    launch_ms: f64,
    launch_to_first_output_ms: f64,
    reconnect_ms: f64,
    idempotent_retry_ms: f64,
    loaded_read_ms: f64,
    cancel_ms: f64,
    final_stdout_offset: u64,
    final_stderr_offset: u64,
    retained_stdout_bytes: usize,
    retained_stderr_bytes: usize,
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    unix_timestamp_seconds: u64,
    operating_system: &'static str,
    architecture: &'static str,
    debug_assertions: bool,
    retention_bytes: u64,
    minimum_load_bytes_per_stream: u64,
    poll_interval_ms: u128,
    samples: Vec<Sample>,
}

fn compile_producer(directory: &Path) -> PathBuf {
    let source = directory.join("producer.rs");
    let executable = directory.join("producer");
    fs::write(&source, PRODUCER).unwrap();
    let output = Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .args(["--edition=2021", "-O"])
        .arg(source)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    executable
}

async fn await_output(backend: &dyn IBackend, run: &RunRef, minimum: u64) -> OutputPage {
    timeout(WATCHDOG, async {
        loop {
            let page = backend
                .read_output(run, OutputCursor::default(), PAGE_BYTES as u32)
                .await
                .unwrap();
            if page.next.stdout >= minimum && page.next.stderr >= minimum {
                return page;
            }
            assert!(
                backend.inspect_run(run).await.unwrap().is_active(),
                "producer stopped before requested load"
            );
            sleep(POLL_INTERVAL).await;
        }
    })
    .await
    .expect("output watchdog expired")
}

async fn sample(provider: &LocalProvider, executable: &Path) -> Sample {
    let backend = provider
        .open(OpenRequest {
            workspace_id: WorkspaceId::generate(),
            config: Default::default(),
            resources: Default::default(),
        })
        .await
        .unwrap();
    let descriptor = backend.descriptor();
    let mut command = CommandSpec::new(executable.to_str().unwrap());
    command.timeout_seconds = Some(25);
    let request = LaunchRequest {
        id: RunId::generate(),
        execution: command.into(),
        output_limit_bytes: RETENTION_BYTES,
    };
    let started = Instant::now();
    let run = backend.start(request.clone()).await.unwrap();
    let launch_ms = started.elapsed().as_secs_f64() * 1000.0;
    await_output(backend.as_ref(), &run, 1).await;
    let launch_to_first_output_ms = started.elapsed().as_secs_f64() * 1000.0;
    drop(backend);
    let started = Instant::now();
    let backend = provider.connect(&descriptor.reference).await.unwrap();
    let reconnect_ms = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    assert_eq!(backend.start(request).await.unwrap().id, run.id);
    let idempotent_retry_ms = started.elapsed().as_secs_f64() * 1000.0;
    await_output(backend.as_ref(), &run, LOAD_BYTES).await;
    let started = Instant::now();
    let page = backend
        .read_output(&run, OutputCursor::default(), PAGE_BYTES as u32)
        .await
        .unwrap();
    let loaded_read_ms = started.elapsed().as_secs_f64() * 1000.0;
    assert!(page.gap, "a stale cursor must expose retention loss");
    assert!(!page.complete);
    assert!(page.stdout.len() + page.stderr.len() <= PAGE_BYTES);
    let observed = page.next;
    assert!(backend.inspect_run(&run).await.unwrap().is_active());
    let started = Instant::now();
    assert_eq!(backend.cancel(&run).await.unwrap(), RunState::Canceled);
    let cancel_ms = started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(backend.inspect_run(&run).await.unwrap(), RunState::Canceled);
    assert_eq!(
        fs::read_to_string(Path::new(&descriptor.root).join("launches")).unwrap(),
        "once\n"
    );

    // Drain after termination to verify stable, monotonic cursors and both stream
    // budgets. No retention gap is acceptable after the first terminal page.
    let mut cursor = OutputCursor::default();
    let (mut stdout_bytes, mut stderr_bytes) = (0, 0);
    let mut first = true;
    loop {
        let page = backend
            .read_output(&run, cursor.clone(), PAGE_BYTES as u32)
            .await
            .unwrap();
        assert_eq!(page.gap, first);
        first = false;
        assert!(page.stdout.len() + page.stderr.len() <= PAGE_BYTES);
        assert!(page.stdout.bytes().all(|byte| byte == b'x'));
        assert!(page.stderr.bytes().all(|byte| byte == b'e'));
        assert!(page.next.stdout >= cursor.stdout && page.next.stderr >= cursor.stderr);
        if !page.gap {
            assert_eq!(page.next.stdout - cursor.stdout, page.stdout.len() as u64);
            assert_eq!(page.next.stderr - cursor.stderr, page.stderr.len() as u64);
        }
        stdout_bytes += page.stdout.len();
        stderr_bytes += page.stderr.len();
        assert!(stdout_bytes <= RETENTION_BYTES as usize / 2);
        assert!(stderr_bytes <= RETENTION_BYTES as usize / 2);
        if !page.complete {
            assert!(
                page.next.stdout > cursor.stdout || page.next.stderr > cursor.stderr,
                "terminal output drain stalled"
            );
        }
        cursor = page.next;
        if page.complete {
            break;
        }
    }
    assert_eq!(stdout_bytes, RETENTION_BYTES as usize / 2);
    assert_eq!(stderr_bytes, RETENTION_BYTES as usize / 2);
    assert!(cursor.stdout >= observed.stdout && cursor.stderr >= observed.stderr);
    let replay = backend
        .read_output(&run, cursor.clone(), PAGE_BYTES as u32)
        .await
        .unwrap();
    assert!(replay.complete && !replay.gap);
    assert!(replay.stdout.is_empty() && replay.stderr.is_empty());
    assert_eq!(replay.next.stdout, cursor.stdout);
    assert_eq!(replay.next.stderr, cursor.stderr);
    Sample {
        launch_ms,
        launch_to_first_output_ms,
        reconnect_ms,
        idempotent_retry_ms,
        loaded_read_ms,
        cancel_ms,
        final_stdout_offset: cursor.stdout,
        final_stderr_offset: cursor.stderr,
        retained_stdout_bytes: stdout_bytes,
        retained_stderr_bytes: stderr_bytes,
    }
}

#[tokio::test]
#[ignore = "local performance qualification: run explicitly with --ignored --nocapture"]
async fn managed_runs_remain_reconnectable_and_cancelable_under_output_load() {
    let report_path = env::var_os("WORKSPACE_SDK_PERF_REPORT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("target/runtime-performance.json")
        });
    // Do not leave an old successful result when this invocation fails.
    match fs::remove_file(&report_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("cannot remove previous performance report: {error}"),
    }
    let directory = tempfile::tempdir().unwrap();
    let executable = compile_producer(directory.path());
    let mut config = LocalConfig::trusted_host(directory.path().join("owned"));
    config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
    let provider = LocalProvider::new(config).unwrap();
    let mut samples = Vec::new();
    for _ in 0..SAMPLES {
        samples.push(
            timeout(WATCHDOG, sample(&provider, &executable))
                .await
                .expect("sample watchdog expired"),
        );
    }
    let report = Report {
        schema_version: 1,
        unix_timestamp_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        operating_system: env::consts::OS,
        architecture: env::consts::ARCH,
        debug_assertions: cfg!(debug_assertions),
        retention_bytes: RETENTION_BYTES,
        minimum_load_bytes_per_stream: LOAD_BYTES,
        poll_interval_ms: POLL_INTERVAL.as_millis(),
        samples,
    };
    let json = serde_json::to_string_pretty(&report).unwrap();
    fs::write(&report_path, &json).unwrap();
    println!("{json}\nPerformance report: {}", report_path.display());
}
