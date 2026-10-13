#![cfg(unix)]
//! Opt-in public-boundary PTY qualification; timings are observations, not baselines.
#[path = "support/pty_fixture.rs"]
#[allow(dead_code)]
mod fixture;
use fixture::{input, run_case};
use serde::Serialize;
use std::{
    env, fs,
    path::Path,
    process::Command,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::time::{sleep, timeout};
use workspace_sdk::{runtime::*, OutputCursor};

#[derive(Serialize)]
struct Sample {
    population: usize,
    launch_ms: f64,
    ready_ms: f64,
    input_delivery_ms: f64,
    resize_observed_ms: f64,
    cancel_ms: f64,
    output_bytes: u64,
    elapsed_to_cancel_ms: f64,
    observed_bytes_per_second: f64,
    retained_bytes: usize,
    supervisor_rss_kib: u64,
    supervisor_open_descriptors: usize,
}
fn elapsed(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}
async fn wait_file(path: &Path) -> Vec<u8> {
    timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(bytes) = fs::read(path) {
                if !bytes.is_empty() {
                    return bytes;
                }
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("workload acknowledgement deadline")
}
fn resource_sample(control_root: &str) -> (u64, usize) {
    let runs = Path::new(control_root).join("runs");
    let identity = fs::read_dir(runs)
        .unwrap()
        .map(|entry| entry.unwrap().path().join("process.json"))
        .find(|path| path.exists())
        .unwrap();
    let identity: serde_json::Value = serde_json::from_slice(&fs::read(identity).unwrap()).unwrap();
    let pid = identity["supervisor_pid"].as_i64().unwrap().to_string();
    let rss = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .unwrap();
    assert!(rss.status.success());
    let rss = String::from_utf8(rss.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let descriptors = if cfg!(target_os = "linux") {
        fs::read_dir(format!("/proc/{pid}/fd")).unwrap().count()
    } else {
        let result = Command::new("lsof")
            .args(["-a", "-p", &pid, "-Ff"])
            .output()
            .unwrap();
        assert!(result.status.success());
        String::from_utf8(result.stdout)
            .unwrap()
            .lines()
            .filter(|line| {
                line.strip_prefix('f')
                    .is_some_and(|fd| fd.chars().next().is_some_and(|c| c.is_ascii_digit()))
            })
            .count()
    };
    (rss, descriptors)
}
fn verify_bytes(bytes: &[u8], from: u64) {
    for (index, byte) in bytes.iter().enumerate() {
        let offset = from + index as u64;
        let position = (offset % 1024) as usize;
        let expected = if position < 8 {
            format!("{:08}", offset / 1024).as_bytes()[position]
        } else if position == 1023 {
            b'\n'
        } else {
            b'x'
        };
        assert_eq!(*byte, expected, "PTY replay mismatch at {offset}");
    }
}

#[tokio::test]
#[ignore = "real local PTY performance qualification"]
async fn qualify_loaded_pty_controls_and_concurrent_sessions() {
    match fs::remove_file("target/runtime-pty-performance.json") {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => panic!("cannot clear previous PTY report: {error}"),
    }
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("pty-performance-workload");
    let compilation = Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .args([
            "--edition=2021",
            "-O",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/support/pty_performance_workload.rs"
            ),
            "-o",
        ])
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compilation.status.success(),
        "{}",
        String::from_utf8_lossy(&compilation.stderr)
    );
    let samples = Arc::new(Mutex::new(Vec::new()));
    for population in [1, 4] {
        for _ in 0..3 {
            let binary = binary.clone();
            let samples = Arc::clone(&samples);
            run_case(population, move |fixtures| async move {
                let mut active = Vec::new();
                for fixture in &fixtures {
                    let host = fixture.host().await;
                    let mut command = fixture.command(&fixture.descriptor.root, &[]);
                    command.program = binary.to_str().unwrap().into();
                    let start = Instant::now();
                    let process = fixture.start(&host, "pty-performance", command).await;
                    let launch_ms = elapsed(start);
                    assert_eq!(
                        wait_file(&Path::new(&fixture.descriptor.root).join("ready")).await,
                        b"ready"
                    );
                    active.push((host, process, start, launch_ms, elapsed(start)));
                }
                // Keep every producer active while each control path is exercised.
                sleep(Duration::from_millis(500)).await;
                let mut tasks = tokio::task::JoinSet::new();
                for (fixture, (host, process, start, launch_ms, ready_ms)) in
                    fixtures.into_iter().zip(active)
                {
                    let samples = Arc::clone(&samples);
                    tasks.spawn(async move {
                        let before = timeout(Duration::from_secs(5), async {
                            loop {
                                let page = host
                                    .read_process_output(&process, OutputCursor::default(), 4096)
                                    .await
                                    .unwrap();
                                if page.next.stdout >= 128 * 1024 {
                                    break page;
                                }
                                sleep(Duration::from_millis(10)).await;
                            }
                        })
                        .await
                        .expect("producer failed to establish load");
                        let control = Instant::now();
                        input(&host, &process, b"i").await;
                        assert_eq!(
                            wait_file(&Path::new(&fixture.descriptor.root).join("input-ack")).await,
                            b"accepted"
                        );
                        let input_delivery_ms = elapsed(control);
                        let control = Instant::now();
                        host.resize_process(&process, RuntimeTerminalSize::new(120, 40).unwrap())
                            .await
                            .unwrap();
                        input(&host, &process, b"r").await;
                        let observed =
                            wait_file(&Path::new(&fixture.descriptor.root).join("resize-ack"))
                                .await;
                        assert_eq!(
                            String::from_utf8(observed)
                                .unwrap()
                                .split_whitespace()
                                .collect::<Vec<_>>(),
                            ["40", "120"]
                        );
                        let resize_observed_ms = elapsed(control);
                        let (supervisor_rss_kib, supervisor_open_descriptors) =
                            resource_sample(&fixture.descriptor.control_root);
                        assert_eq!(
                            host.inspect_process(&process).await.unwrap(),
                            RuntimeProcessState::Running
                        );
                        let control = Instant::now();
                        assert_eq!(
                            host.cancel_process(&process).await.unwrap(),
                            RuntimeProcessState::Exited { code: 130 }
                        );
                        let cancel_ms = elapsed(control);
                        let elapsed_to_cancel_ms = elapsed(start);
                        let mut cursor = OutputCursor::default();
                        let mut retained_bytes = 0;
                        loop {
                            let page = host
                                .read_process_output(&process, cursor.clone(), 4096)
                                .await
                                .unwrap();
                            assert!(page.stderr.is_empty());
                            assert_eq!(page.next.stderr, 0);
                            let from = page.next.stdout - page.stdout.len() as u64;
                            if retained_bytes > 0 {
                                assert!(!page.gap);
                                assert_eq!(from, cursor.stdout);
                            } else {
                                assert!(page.gap, "expired cursor must report retention loss");
                            }
                            verify_bytes(page.stdout.as_bytes(), from);
                            retained_bytes += page.stdout.len();
                            cursor = page.next;
                            if page.complete {
                                break;
                            }
                        }
                        assert!(
                            cursor.stdout > before.next.stdout,
                            "output did not advance during controls"
                        );
                        assert_eq!(retained_bytes, 128 * 1024);
                        let settled = host
                            .read_process_output(&process, cursor.clone(), 4096)
                            .await
                            .unwrap();
                        assert!(settled.complete && !settled.gap);
                        assert!(settled.stdout.is_empty() && settled.stderr.is_empty());
                        assert_eq!(settled.next.stdout, cursor.stdout);
                        assert_eq!(settled.next.stderr, cursor.stderr);
                        samples.lock().unwrap().push(Sample {
                            population,
                            launch_ms,
                            ready_ms,
                            input_delivery_ms,
                            resize_observed_ms,
                            cancel_ms,
                            output_bytes: cursor.stdout,
                            elapsed_to_cancel_ms,
                            observed_bytes_per_second: cursor.stdout as f64
                                / (elapsed_to_cancel_ms / 1000.0),
                            retained_bytes,
                            supervisor_rss_kib,
                            supervisor_open_descriptors,
                        });
                    });
                }
                while let Some(result) = tasks.join_next().await {
                    result.unwrap();
                }
            })
            .await;
        }
    }
    let report = serde_json::json!({ "schema_version": 1, "unix_timestamp_seconds": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(), "os": env::consts::OS, "architecture": env::consts::ARCH, "debug_assertions": cfg!(debug_assertions), "samples": *samples.lock().unwrap() });
    let report = serde_json::to_string_pretty(&report).unwrap();
    fs::create_dir_all("target").unwrap();
    fs::write("target/runtime-pty-performance.json", &report).unwrap();
    println!("{report}");
}
