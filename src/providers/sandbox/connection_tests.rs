use super::*;
use crate::ports::MAX_SOCKET_PATH;
use axum::{routing::post, Json, Router};
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use tokio::io::{AsyncBufReadExt, BufReader};

const VM: &str = "bx_23456789";

fn host_key_line(key: &[u8; 32]) -> String {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&11u32.to_be_bytes());
    bytes.extend_from_slice(b"ssh-ed25519");
    bytes.extend_from_slice(&32u32.to_be_bytes());
    bytes.extend_from_slice(key);
    format!(
        "ssh-ed25519 {}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// The wrapper, run as sshd runs it: leading its own session, under a
/// parent whose name starts like sshd's.
#[cfg(target_os = "linux")]
fn bound(command: &str, shell: &str) -> std::process::Output {
    let place = tempfile::tempdir().unwrap();
    let sshd = place.path().join("sshd-fixture");
    let shell = String::from_utf8(
        std::process::Command::new("sh")
            .args(["-c", &format!("command -v {shell}")])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    std::os::unix::fs::symlink(shell.trim(), &sshd).unwrap();
    // `exit` keeps the shell from replacing itself with setsid, so it stays
    // the wrapper's parent; setsid does not fork outside job control.
    std::process::Command::new(&sshd)
        .args([
            "-c",
            r#"setsid sh -c "$1" workspace-sdk "$2" "$3"; exit $?"#,
            "sshd-fixture",
            SESSION_BOUND,
            command,
            WATCH,
        ])
        .output()
        .unwrap()
}

#[test]
// The wrapper reads Linux's `/proc`.
#[cfg(target_os = "linux")]
fn a_script_with_its_own_quotes_reaches_sh_whole() {
    // The machine's sshd hands the command to the account's login shell.
    for shell in ["bash", "sh"] {
        let said = bound("echo 'a  b'; echo \"$(echo c)\"; exit 3", shell);
        assert_eq!(
            String::from_utf8_lossy(&said.stdout),
            "a  b\nc\n",
            "{shell}"
        );
        assert_eq!(said.status.code(), Some(3), "{shell}");
        // A command that looks like an option is still a command.
        assert_eq!(bound("-x", shell).status.code(), Some(127), "{shell}");
    }
}

#[test]
// The wrapper reads Linux's `/proc`.
#[cfg(target_os = "linux")]
fn what_a_command_leaves_in_its_session_ends_with_it() {
    let started = std::time::Instant::now();
    let said = bound(
        "trap '' TERM; sleep 4701 & timeout 4702 sleep 4702 & \
         (trap '' TERM; sleep 4703) & echo started",
        "sh",
    );
    assert_eq!(String::from_utf8_lossy(&said.stdout), "started\n");
    assert_eq!(said.status.code(), Some(0));
    assert!(started.elapsed() < Duration::from_secs(10));
    for marker in ["4701", "4702", "4703"] {
        let mut left = running(marker);
        for _ in 0..50 {
            if !left {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
            left = running(marker);
        }
        assert!(!left, "{marker} outlived its command");
    }
}

#[test]
// The wrapper reads Linux's `/proc`.
#[cfg(target_os = "linux")]
fn a_command_outside_an_sshd_session_is_refused() {
    let script = format!(
        "exec sh -c {} workspace-sdk {} {}",
        quote(SESSION_BOUND),
        quote("echo ran"),
        quote(WATCH)
    );
    let said = std::process::Command::new("sh")
        .args(["-c", &script])
        .output()
        .unwrap();
    assert_eq!(said.status.code(), Some(255));
    assert!(said.stdout.is_empty());
    assert!(String::from_utf8_lossy(&said.stderr).contains("does not lead its session"));
    // Leading its session under anything but sshd: its connection is gone.
    let said = std::process::Command::new("setsid")
        .args([
            "-w",
            "sh",
            "-c",
            SESSION_BOUND,
            "workspace-sdk",
            "echo ran",
            WATCH,
        ])
        .output()
        .unwrap();
    assert_eq!(said.status.code(), Some(255));
    assert!(said.stdout.is_empty());
    assert!(String::from_utf8_lossy(&said.stderr).contains("already ended"));
}

#[test]
fn only_one_ed25519_host_key_is_pinned() {
    let line = host_key_line(&[7; 32]);
    let encoded = line.split(' ').nth(1).unwrap();
    assert_eq!(
        known_host(Some(&line)),
        Some(format!("{HOST_ALIAS} ssh-ed25519 {encoded}\n"))
    );
    assert_eq!(
        known_host(Some(&format!("{line} comment"))),
        Some(format!("{HOST_ALIAS} ssh-ed25519 {encoded}\n"))
    );
    let short = format!(
        "ssh-ed25519 {}",
        base64::engine::general_purpose::STANDARD.encode([0u8; 19])
    );
    for bad in [
        None,
        Some(""),
        Some("ssh-ed25519"),
        Some("ssh-ed25519 AAAA"),
        Some(short.as_str()),
        Some("ssh-rsa AAAA"),
        Some("ssh-ed25519 not*base64"),
        Some("ssh-ed25519 AAAA\nevil ssh-ed25519 AAAA"),
    ] {
        assert_eq!(known_host(bad), None, "{bad:?}");
    }
    let long = "A".repeat(260);
    assert_eq!(known_host(Some(&format!("ssh-ed25519 {long}"))), None);
}

#[test]
fn a_forward_takes_only_a_short_plain_absolute_path() {
    assert!(SocketForward::new("/tmp/a.sock").is_ok());
    for bad in [
        "relative.sock".to_owned(),
        "/tmp/a:b.sock".into(),
        "/tmp/a\nb.sock".into(),
        format!("/{}", "x".repeat(MAX_SOCKET_PATH)),
    ] {
        assert!(SocketForward::new(bad.clone()).is_err(), "{bad:?}");
    }
}

#[tokio::test]
async fn a_long_output_keeps_its_end() {
    let bytes: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
    let tail = read_tail(&bytes[..], 1000).await.unwrap();
    assert_eq!(tail, bytes[9000..]);
    assert_eq!(read_tail(&b"short"[..], 1000).await.unwrap(), b"short");
}

/// A fake Boat answering `sshkey` with `answer`, writing the key it was
/// given to `authorized`. It answers until dropped.
struct Boat {
    api: Api,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Boat {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn boat(answer: Value, authorized: Option<std::path::PathBuf>) -> Boat {
    let router = Router::new().route(
        "/api/v1/sandboxes/{id}/sshkey",
        post(move |Json(request): Json<Value>| {
            if let Some(authorized) = &authorized {
                // Written in place, so a mode a test set is kept.
                let _ = std::fs::OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(authorized)
                    .and_then(|mut file| {
                        std::io::Write::write_all(
                            &mut file,
                            request["key"].as_str().unwrap().as_bytes(),
                        )
                    });
            }
            let answer = answer.clone();
            async move { Json(answer) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api/v1", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let api = Api::new(
        sandbox_client::ClientConfig::new("fixture-token")
            .unwrap()
            .with_base_path(&base)
            .unwrap(),
    )
    .unwrap();
    Boat { api, task }
}

#[tokio::test]
async fn an_authorization_without_everything_to_check_the_machine_is_refused() {
    let good = json!({
        "ok": true, "type": "sandbox.sshkey", "success": true, "sshUser": "user",
        "sshEndpoint": "127.0.0.1:22", "hostKey": host_key_line(&[1; 32]),
    });
    let fake = boat(good.clone(), None).await;
    assert!(authorize(&fake.api, VM, "key".into()).await.is_ok());
    for (field, value) in [
        ("ok", json!(false)),
        ("success", json!(false)),
        ("success", Value::Null),
        ("sshUser", json!("root")),
        ("sshEndpoint", Value::Null),
        ("sshEndpoint", json!("machine.example:22")),
        ("hostKey", Value::Null),
        ("hostKey", json!("ssh-rsa AAAA")),
    ] {
        let mut answer = good.clone();
        answer[field] = value.clone();
        let fake = boat(answer, None).await;
        assert!(
            authorize(&fake.api, VM, "key".into()).await.is_err(),
            "{field}: {value}"
        );
    }
}

/// A real OpenSSH server for the machine, and a fake Boat that authorizes
/// keys on it. It needs `/usr/sbin/sshd`, `python3` and an account named
/// `user` with a usable (not locked) password field, so the tests using it
/// run only when asked: `cargo test --features boat connection -- --ignored`.
struct Machine {
    place: tempfile::TempDir,
    _sshd: tokio::process::Child,
    boat: Boat,
}
impl Machine {
    async fn start(allow_forwarding: bool) -> Self {
        let place = tempfile::tempdir().unwrap();
        // sshd reads the authorized keys as `user`.
        std::fs::set_permissions(place.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = |name: &str| place.path().join(name);
        let made = std::process::Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-f"])
            .arg(path("host"))
            .status()
            .unwrap();
        assert!(made.success());
        let host_key = std::fs::read_to_string(path("host.pub")).unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        std::fs::write(path("authorized"), "").unwrap();
        std::fs::write(
            path("sshd_config"),
            format!(
                "Port {port}\nListenAddress 127.0.0.1\nHostKey {}\nAuthorizedKeysFile {}\n\
                 StrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\n\
                 UsePAM no\nPidFile none\nAllowStreamLocalForwarding {}\n\
                 Subsystem sftp /usr/lib/openssh/sftp-server\n",
                path("host").display(),
                path("authorized").display(),
                if allow_forwarding { "yes" } else { "no" },
            ),
        )
        .unwrap();
        let mut sshd = tokio::process::Command::new("/usr/sbin/sshd")
            .args(["-D", "-e", "-f"])
            .arg(path("sshd_config"))
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        while tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_err()
        {
            assert!(sshd.try_wait().unwrap().is_none(), "sshd did not start");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let answer = json!({
            "ok": true, "type": "sandbox.sshkey", "success": true, "sshUser": "user",
            "sshEndpoint": format!("127.0.0.1:{port}"), "hostKey": host_key.trim(),
        });
        let boat = boat(answer, Some(path("authorized"))).await;
        Self {
            place,
            _sshd: sshd,
            boat,
        }
    }
    async fn connect(&self, forward: Option<SocketForward>) -> Result<Arc<SshConnection>> {
        SshConnection::connect(&self.boat.api, VM, forward).await
    }
}

/// Whether a process whose command line holds `marker` runs on this host,
/// which is also the machine here.
fn running(marker: &str) -> bool {
    let listed = std::process::Command::new("ps")
        .args(["-eo", "args"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&listed.stdout)
        .lines()
        .any(|line| line.starts_with("sleep") && line.contains(marker))
}

async fn gone(marker: &str) -> bool {
    for _ in 0..50 {
        if !running(marker) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

#[tokio::test]
#[ignore = "needs a local sshd and an account named user"]
async fn a_connection_streams_runs_and_serves_files() {
    let machine = Machine::start(true).await;
    let connection = machine.connect(None).await.unwrap();
    let private = connection.directory.path().to_owned();
    assert_eq!(connection.forwarded_socket(), None);

    let ran = connection
        .run("echo 'it'\"'\"'s'; whoami", None, Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(
        (&ran.end, ran.output.as_str()),
        (&ProcessEnd::Exited(0), "it's\nuser\n")
    );
    let failed = connection
        .run(
            "echo partial; echo why >&2; exit 7",
            None,
            Duration::from_secs(10),
        )
        .await
        .unwrap();
    assert_eq!(
        (&failed.end, failed.output.as_str(), failed.stderr.as_str()),
        (&ProcessEnd::Exited(7), "partial\n", "why\n")
    );
    let input = tempfile::tempfile().unwrap();
    std::io::Write::write_all(&mut &input, b"from a file\n").unwrap();
    std::io::Seek::rewind(&mut &input).unwrap();
    let echoed = connection
        .run("cat", Some(input), Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(echoed.output, "from a file\n");

    let mut stream = connection
        .clone()
        .stream(
            "IFS= read -r t; echo \"first:$t\"; cat; exit 4",
            Some(b"SENTINEL_SECRET"),
        )
        .await
        .unwrap();
    stream.input.write_all(b"after\n").await.unwrap();
    drop(stream.input);
    let mut said = String::new();
    stream.output.read_to_string(&mut said).await.unwrap();
    assert_eq!(said, "first:SENTINEL_SECRET\nafter\n");
    assert_eq!(
        stream.process.wait().await.unwrap().end,
        ProcessEnd::Exited(4)
    );
    assert!(matches!(
        connection.clone().stream("true", Some(b"a\nb")).await,
        Err(Error::InvalidInput(_))
    ));

    // SSH_FXP_INIT, version 3, answered by SSH_FXP_VERSION.
    let mut files = connection.clone().files().await.unwrap();
    files
        .to_machine
        .write_all(&[0, 0, 0, 5, 1, 0, 0, 0, 3])
        .await
        .unwrap();
    let mut header = [0u8; 5];
    files.from_machine.read_exact(&mut header).await.unwrap();
    assert_eq!(header[4], 2);
    drop(files);

    drop(stream.process);
    drop(connection);
    assert!(!private.exists());
}

#[tokio::test]
#[ignore = "needs a local sshd, python3 and an account named user"]
async fn a_forwarded_socket_is_bound_when_connected_and_removed_after() {
    let machine = Machine::start(true).await;
    let local = machine.place.path().join("local.sock");
    let listening = tokio::net::UnixListener::bind(&local).unwrap();
    let echo = tokio::spawn(async move {
        loop {
            let (socket, _) = listening.accept().await.unwrap();
            tokio::spawn(async move {
                let (read, mut write) = socket.into_split();
                let mut lines = BufReader::new(read).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    write
                        .write_all(format!("local:{line}\n").as_bytes())
                        .await
                        .unwrap();
                }
            });
        }
    });
    let connection = machine
        .connect(Some(SocketForward::new(&local).unwrap()))
        .await
        .unwrap();
    let remote = connection.forwarded_socket().unwrap().to_owned();
    // Used at once: it is bound before the connection is handed out.
    let relayed = connection
        .run(
            &format!(
                "python3 -c 'import socket,sys; s=socket.socket(socket.AF_UNIX); \
                 s.connect(sys.argv[1]); s.sendall(b\"ping\\n\"); \
                 print(s.makefile().readline(), end=\"\")' {remote}"
            ),
            None,
            Duration::from_secs(10),
        )
        .await
        .unwrap();
    assert_eq!(relayed.output, "local:ping\n");
    let directory = std::path::Path::new(&remote).parent().unwrap().to_owned();
    assert!(directory.exists());
    drop(connection);
    for _ in 0..50 {
        if !directory.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(!directory.exists(), "the forward's directory outlived it");
    echo.abort();
}

#[tokio::test]
#[ignore = "needs a local sshd and an account named user"]
async fn a_refused_key_or_forward_fails_with_what_ssh_said() {
    let machine = Machine::start(false).await;
    let local = machine.place.path().join("local.sock");
    let _listening = tokio::net::UnixListener::bind(&local).unwrap();
    let refused = machine
        .connect(Some(SocketForward::new(&local).unwrap()))
        .await;
    let Err(Error::Provider { message, .. }) = refused else {
        panic!("a refused forward connected");
    };
    assert!(message.contains("forward was refused"), "{message}");

    let authorized = machine.place.path().join("authorized");
    std::fs::set_permissions(&authorized, std::fs::Permissions::from_mode(0o000)).unwrap();
    let Err(Error::Provider { message, .. }) = machine.connect(None).await else {
        panic!("a refused key connected");
    };
    assert!(message.contains("Permission denied"), "{message}");
}

#[tokio::test]
#[ignore = "needs a local sshd and an account named user"]
async fn a_dropped_or_timed_out_command_ends_on_the_machine() {
    let machine = Machine::start(true).await;
    let connection = machine.connect(None).await.unwrap();

    let stream = connection
        .clone()
        .stream("sleep 4301 & sleep 4302", None)
        .await
        .unwrap();
    for _ in 0..50 {
        if running("4302") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(running("4301") && running("4302"));
    drop(stream);
    assert!(
        gone("4301").await && gone("4302").await,
        "a dropped stream kept running"
    );

    let started = std::time::Instant::now();
    assert!(matches!(
        connection
            .run("sleep 4303", None, Duration::from_millis(500))
            .await,
        Err(Error::Timeout(_))
    ));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(gone("4303").await, "a timed-out command kept running");

    // A command that ignores TERM, and one that left its process group,
    // still end with their dropped stream.
    let stream = connection
        .clone()
        .stream("trap '' TERM; timeout 4305 sleep 4305 & sleep 4306", None)
        .await
        .unwrap();
    for _ in 0..50 {
        if running("4306") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    drop(stream);
    for _ in 0..100 {
        if !running("4305") && !running("4306") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        !running("4305") && !running("4306"),
        "a dropped stream kept running"
    );
}

#[tokio::test]
#[ignore = "needs a local sshd and an account named user"]
async fn with_its_master_gone_a_connection_refuses_new_commands_and_files() {
    let machine = Machine::start(true).await;
    let connection = machine.connect(None).await.unwrap();
    let master = connection
        .master
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .id()
        .unwrap();
    std::process::Command::new("kill")
        .args(["-9", &master.to_string()])
        .status()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let Err(Error::Provider { message, .. }) = connection
        .run("echo still", None, Duration::from_secs(10))
        .await
    else {
        panic!("a command ran without its master");
    };
    assert!(message.contains("lost"), "{message}");
    assert!(connection.clone().files().await.is_err());
    // Through the control socket alone, a session fails rather than signing
    // in on its own.
    let mut alone = connection.session();
    alone.arg(connection.target()).arg("true");
    assert_eq!(alone.status().await.unwrap().code(), Some(255));
}
