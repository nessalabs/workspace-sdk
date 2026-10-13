//! Standalone real-terminal workload. Compiled by the integration test with rustc.
use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, BufRead, Read, Write},
    os::unix::net::UnixStream,
    process::{self, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

extern "C" {
    fn isatty(fd: i32) -> i32;
    fn getpgrp() -> i32;
    fn tcgetpgrp(fd: i32) -> i32;
    fn signal(number: i32, disposition: usize) -> usize;
}

fn terminal() {
    // SAFETY: read-only queries of the fixture's own standard descriptors/group.
    unsafe {
        assert_eq!((isatty(0), isatty(1), isatty(2)), (1, 1, 1));
        assert_eq!(tcgetpgrp(0), getpgrp());
    }
    assert!(Command::new("/bin/stty")
        .args(["-echo", "-icanon", "min", "1", "time", "0", "-opost"])
        .stdin(Stdio::inherit())
        .status()
        .unwrap()
        .success());
}

fn out(bytes: &[u8]) {
    io::stdout().write_all(bytes).unwrap();
    io::stdout().flush().unwrap();
}
fn err(bytes: &[u8]) {
    io::stderr().write_all(bytes).unwrap();
    io::stderr().flush().unwrap();
}

fn interactive(check_environment: bool) {
    terminal();
    let mut launches = OpenOptions::new()
        .create(true)
        .append(true)
        .open("launches")
        .unwrap();
    launches.write_all(b"launch\n").unwrap();
    launches.sync_all().unwrap();
    out(b"tty:111\nforeground:yes\n");
    out(format!("cwd:{}\n", env::current_dir().unwrap().display()).as_bytes());
    if check_environment {
        assert!(env::var_os("PATH").is_none());
        assert_eq!(env::var("PTY_PUBLIC_VALUE").unwrap(), "visible-fixture");
        // Never print the supplied secret, including on an assertion failure.
        assert!(env::var("PTY_PRIVATE_VALUE").is_ok_and(|v| v == "pty-env-private-4f712"));
        out(b"env:ok\n");
    }
    err(b"stderr:ready\n");
    out(b"ready\n");
    let mut input = io::stdin().lock();
    loop {
        let mut line = String::new();
        assert!(
            input.read_line(&mut line).unwrap() > 0,
            "unexpected terminal EOF"
        );
        match line.as_str() {
            "pty-input-private-992c1\n" => out(b"input:accepted\n"),
            "size\n" => {
                let size = Command::new("/bin/stty")
                    .arg("size")
                    .stdin(Stdio::inherit())
                    .output()
                    .unwrap();
                assert!(size.status.success());
                let text = String::from_utf8(size.stdout).unwrap();
                let dimensions: Vec<_> = text.split_whitespace().collect();
                assert_eq!(dimensions.len(), 2);
                out(format!("size:{}x{}\n", dimensions[1], dimensions[0]).as_bytes());
            }
            "quit\n" => {
                out(b"stdout:done\n");
                err(b"stderr:done\n");
                return;
            }
            _ => panic!("unexpected input command (content omitted)"),
        }
    }
}

fn burst() {
    terminal();
    out(b"stdout-prefix\n");
    err(b"stderr-prefix\n");
    let body: Vec<u8> = (0..300_000)
        .map(|index| b'a' + (index % 26) as u8)
        .collect();
    out(&body);
    out(b"\nstdout-tail\n");
    err(b"stderr-tail\n");
}

fn hold_terminal(socket: &str) {
    // SAFETY: this dedicated child ignores only its own HUP/TERM dispositions.
    // SIG_IGN is the POSIX sentinel 1 on the supported macOS/Linux hosts.
    unsafe {
        signal(1, 1);
        signal(15, 1);
    }
    let mut control = UnixStream::connect(socket).unwrap();
    writeln!(control, "{}", process::id()).unwrap();
    control.flush().unwrap();
    fs::write("descendant-ready", b"ready").unwrap();
    // The test retains this endpoint. Unwind/timeout closes it, causing the
    // fixture to terminate itself rather than signalling a cached numeric PID.
    thread::spawn(move || {
        let mut byte = [0];
        let _ = control.read(&mut byte);
        process::exit(0);
    });
    thread::sleep(Duration::from_secs(3));
    fs::write("descendant-escaped", b"cleanup failed").unwrap();
    thread::sleep(Duration::from_secs(30));
}

fn trickle_client(directory: &str, socket: &str) {
    // A separate executable owns cwd changes; the async test host never chdirs.
    env::set_current_dir(directory).unwrap();
    let mut terminal = UnixStream::connect("pty.sock").unwrap();
    terminal.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
    terminal.set_read_timeout(Some(Duration::from_millis(40))).unwrap();
    let mut control = UnixStream::connect(socket).unwrap();
    terminal.write_all(b"{").unwrap();
    writeln!(control, "connected").unwrap();
    control.flush().unwrap();
    let started = Instant::now();
    loop {
        thread::sleep(Duration::from_millis(40));
        let write_failed = terminal.write_all(b" ").is_err();
        let mut response = [0; 4096];
        // Consume the owner's error acknowledgement and observe EOF. On macOS,
        // writes alone can remain buffered after the peer closes its socket.
        let closed = match terminal.read(&mut response) {
            Ok(0) => true,
            Ok(_) => false,
            Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => false,
            Err(_) => true,
        };
        if write_failed || closed {
            writeln!(control, "closed:{}", started.elapsed().as_millis()).unwrap();
            control.flush().unwrap();
            return;
        }
        assert!(started.elapsed() < Duration::from_secs(5), "slow frame never expired");
    }
}

fn main() {
    // Independent backstop for a broken owner or test harness.
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(30));
        process::exit(120);
    });
    let args: Vec<_> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("interactive") => interactive(true),
        Some("control") => interactive(false),
        Some("burst") => burst(),
        Some("hold") => hold_terminal(&args[1]),
        Some("owner-tree") => {
            terminal();
            let mut launches = OpenOptions::new().create(true).append(true).open("launches").unwrap();
            launches.write_all(b"launch\n").unwrap();
            launches.sync_all().unwrap();
            let child = Command::new(env::current_exe().unwrap())
                .args(["hold", &args[1]])
                .spawn()
                .unwrap();
            // Both leader and descendant retain an independent test-owned
            // channel; closing either makes that process exit on test failure.
            hold_terminal(&args[1]);
            drop(child);
        }
        Some("trickle-client") => trickle_client(&args[1], &args[2]),
        Some("hold-leader") => {
            terminal();
            let child = Command::new(env::current_exe().unwrap())
                .args(["hold", &args[1]])
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            while !std::path::Path::new("descendant-ready").exists() {
                assert!(Instant::now() < deadline, "descendant did not become ready");
                thread::sleep(Duration::from_millis(10));
            }
            out(b"leader-complete\n");
            drop(child); // The PTY owner must settle the descendant after this exit.
        }
        _ => panic!("unknown fixture mode"),
    }
}
