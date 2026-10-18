//! Actual child/stream collection boundaries, not GPU driver qualification or
//! descendant cleanup. Every fixture command uses shell builtins or execs its
//! sole workload; no PATH mutation, detached grandchildren or numeric PID signals.
use super::collect_gpus;
use std::{
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::net::UnixStream,
    },
    process::{Child, ChildStdout, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

fn fixture(script: &str) -> Child {
    Command::new("/bin/sh")
        .args(["-c", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn stream_fixture(script: &str, writer: &UnixStream, reader: UnixStream) -> Child {
    let child_output: OwnedFd = writer.try_clone().unwrap().into();
    let mut child = Command::new("/bin/sh")
        .args(["-c", script])
        .stdin(Stdio::null())
        .stdout(Stdio::from(child_output))
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let reader: OwnedFd = reader.into();
    child.stdout = Some(ChildStdout::from(reader));
    child
}

#[test]
fn exited_child_with_an_open_output_writer_does_not_acknowledge_partial_gpu_output() {
    let (reader, writer) = UnixStream::pair().unwrap();
    let mut child = stream_fixture("printf 'GPU fixture, 42\\n'", &writer, reader);
    // Establish the exited-leader precondition before applying the collector's
    // short budget; delayed startup must not turn the old bug into a timeout pass.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("fixture did not establish an exited leader");
        }
        thread::sleep(Duration::from_millis(5));
    }
    let (actual, elapsed) = thread::scope(|scope| {
        let (done, finished) = mpsc::channel();
        // Holding this writer models the exact open-stream boundary that an
        // inherited descriptor causes after a probe leader exits. The fail-safe
        // bounds old blocking-read execution; it is not the collection deadline.
        let holder = scope.spawn(move || {
            let _ = finished.recv_timeout(Duration::from_secs(2));
            drop(writer);
        });
        let started = Instant::now();
        let actual = collect_gpus(child, Duration::from_millis(100));
        let elapsed = started.elapsed();
        let _ = done.send(());
        holder.join().unwrap();
        (actual, elapsed)
    });
    // A broad deadline check distinguishes bounded collection from waiting for
    // the two-second fail-safe and only then noticing expiry. Not a benchmark.
    assert!(
        elapsed < Duration::from_secs(1),
        "collector waited {elapsed:?}"
    );
    assert!(
        actual.is_empty(),
        "without EOF the observation is incomplete"
    );
}

#[test]
fn valid_gpu_output_larger_than_the_stream_send_buffer_is_collected_while_running() {
    let (reader, writer) = UnixStream::pair().unwrap();
    let requested: libc::c_int = 1024;
    let mut actual: libc::c_int = 0;
    let mut length = std::mem::size_of_val(&actual) as libc::socklen_t;
    // Constrain only this disposable fixture socket. No process-wide setting.
    assert_eq!(
        unsafe {
            libc::setsockopt(
                writer.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                (&requested as *const libc::c_int).cast(),
                std::mem::size_of_val(&requested) as libc::socklen_t,
            )
        },
        0
    );
    assert_eq!(
        unsafe {
            libc::getsockopt(
                writer.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                (&mut actual as *mut libc::c_int).cast(),
                &mut length,
            )
        },
        0
    );
    assert_eq!(length as usize, std::mem::size_of_val(&actual));
    let rows = 4000;
    let output_bytes = rows * b"GPU, 1\n".len();
    assert!(actual > 0 && (actual as usize) < output_bytes);
    assert!(output_bytes <= 65_536);
    let child = stream_fixture(
        "i=0; while [ \"$i\" -lt 4000 ]; do printf 'GPU, 1\\n'; i=$((i + 1)); done",
        &writer,
        reader,
    );
    drop(writer); // Only the actual producer now owns the stream's writer.
    let observed = collect_gpus(child, Duration::from_secs(2));
    assert_eq!(observed.len(), rows);
    assert!(observed
        .iter()
        .all(|gpu| gpu.model == "GPU" && gpu.memory_mib == 1));
}

#[test]
fn complete_gpu_csv_requires_successful_exit_and_valid_utf8() {
    let observed = collect_gpus(
        fixture("printf 'GPU fixture, 42\\n'"),
        Duration::from_secs(2),
    );
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].model, "GPU fixture");
    assert_eq!(observed[0].memory_mib, 42);
    assert!(collect_gpus(
        fixture("printf 'GPU fixture, 42\\n'; exit 7"),
        Duration::from_secs(2)
    )
    .is_empty());
    assert!(collect_gpus(
        fixture("printf 'GPU fixture, 42\\n'; printf '\\377'"),
        Duration::from_secs(2)
    )
    .is_empty());
}

#[test]
fn gpu_output_byte_limit_accepts_exact_boundary_and_rejects_one_extra_byte() {
    // Seven CSV bytes followed by padding: without the byte guard, the first
    // device remains parseable even when the final row is merely whitespace.
    let exact = collect_gpus(
        fixture("printf 'GPU, 1\\n'; printf '%65529s' ''"),
        Duration::from_secs(2),
    );
    assert_eq!(exact.len(), 1);
    assert_eq!(exact[0].model, "GPU");
    assert_eq!(exact[0].memory_mib, 1);
    let over = collect_gpus(
        fixture("printf 'GPU, 1\\n'; printf '%65530s' ''"),
        Duration::from_secs(2),
    );
    assert!(
        over.is_empty(),
        "over-limit output cannot establish a device"
    );
}

#[test]
fn nonexiting_direct_gpu_probe_times_out_as_unknown() {
    // exec leaves one directly owned child, rather than an orphanable shell
    // parent/sleep pair. An empty observation is not proof GPUs are absent.
    let child = fixture("exec /bin/sleep 2");
    let started = Instant::now();
    let observed = collect_gpus(child, Duration::from_millis(25));
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(1),
        "collector waited {elapsed:?}"
    );
    assert!(observed.is_empty());
}
