use super::*;
use crate::ports::MAX_SOCKET_PATH;
use std::os::unix::fs::PermissionsExt;
use tokio::io::{AsyncBufReadExt, BufReader};

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

/// A real OpenSSH server for the machine, reached directly or through a
/// local tunnel, and a route that authorizes each connection's key on it. It needs `/usr/sbin/sshd`, `python3` and an account named
/// `user` with a usable (not locked) password field, so the tests using it
/// run only when asked: `cargo test --features boat connection -- --ignored`, on Linux.
struct Machine {
    place: tempfile::TempDir,
    _sshd: tokio::process::Child,
    port: u16,
    host_key: String,
    tunnel: bool,
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
        Self {
            place,
            _sshd: sshd,
            port,
            host_key: host_key.trim().to_owned(),
            tunnel: false,
        }
    }
    /// The same machine, reached through a local tunnel instead.
    fn tunnelled(mut self) -> Self {
        self.tunnel = true;
        self
    }
    async fn connect(&self, forward: Option<SocketForward>) -> Result<Arc<SshConnection>> {
        let authorized = self.place.path().join("authorized");
        let (port, host_key, tunnel) = (self.port, self.host_key.clone(), self.tunnel);
        SshConnection::connect(
            |directory| async move {
                let key = KeyPair::generate("test").await?;
                // Written in place, so a mode a test set is kept.
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(&authorized)
                {
                    std::io::Write::write_all(&mut file, key.public().as_bytes())?;
                }
                let endpoint: std::net::SocketAddr = ([127, 0, 0, 1], port).into();
                if !tunnel {
                    return Ok(Route {
                        user: "user".into(),
                        key,
                        dial: Dial::Direct { endpoint, host_key },
                        guard: None,
                    });
                }
                let socket = directory.join("tunnel.sock");
                let listener = tokio::net::UnixListener::bind(&socket)?;
                let task = tokio::spawn(async move {
                    while let Ok((mut local, _)) = listener.accept().await {
                        tokio::spawn(async move {
                            if let Ok(mut remote) = tokio::net::TcpStream::connect(endpoint).await {
                                let _ =
                                    tokio::io::copy_bidirectional(&mut local, &mut remote).await;
                            }
                        });
                    }
                });
                Ok(Route {
                    user: "user".into(),
                    key,
                    dial: Dial::Tunnel {
                        socket,
                        host: "machine.test".into(),
                    },
                    guard: Some(Box::new(AbortOnDrop(task))),
                })
            },
            forward,
        )
        .await
    }
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
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

/// Through a tunnel the host key is learnt by the master and held to by
/// every later session, each of which dials the tunnel again.
#[tokio::test]
#[ignore = "needs a local sshd, python3 and an account named user"]
async fn a_tunnelled_connection_runs_forwards_and_pins_what_it_learnt() {
    let machine = Machine::start(true).await.tunnelled();
    let local = machine.place.path().join("local.sock");
    let _listening = tokio::net::UnixListener::bind(&local).unwrap();
    let connection = machine
        .connect(Some(SocketForward::new(&local).unwrap()))
        .await
        .unwrap();
    let ran = connection
        .run("echo through", None, Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(
        (ran.end, ran.output.as_str()),
        (ProcessEnd::Exited(0), "through\n")
    );
    assert!(connection.forwarded_socket().is_some());
    let hosts = connection.dialer.directory.path().join("hosts");
    let learnt = std::fs::read_to_string(&hosts).unwrap();
    assert!(learnt.contains("ssh-ed25519"), "{learnt}");
    // Another key in its place: the next session must refuse the machine.
    let other = machine.place.path().join("other");
    assert!(std::process::Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(&other)
        .status()
        .unwrap()
        .success());
    let forged = std::fs::read_to_string(other.with_extension("pub")).unwrap();
    let forged = forged
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    std::fs::write(&hosts, format!("{HOST_ALIAS} {forged}\n")).unwrap();
    let refused = connection
        .run("echo through", None, Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(refused.end, ProcessEnd::Lost);
}

#[test]
fn a_tunnel_path_with_anything_a_shell_reads_is_refused() {
    assert!(plain_path(Path::new("/tmp/wsdk-ssh-AbC123/tunnel.sock")));
    for bad in [
        "relative.sock",
        "/tmp/a b.sock",
        "/tmp/a;b",
        "/tmp/$(x)",
        "/tmp/a'b",
    ] {
        assert!(!plain_path(Path::new(bad)), "{bad}");
    }
}

#[tokio::test]
#[ignore = "needs a local sshd and an account named user"]
async fn a_connection_streams_runs_and_serves_files() {
    let machine = Machine::start(true).await;
    let connection = machine.connect(None).await.unwrap();
    let private = connection.dialer.directory.path().to_owned();
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

/// A file sent through `run`, such as a build, is compressed on the way;
/// a stream, whose bytes are small and live, is not.
#[tokio::test]
async fn only_a_run_with_input_is_compressed() {
    let directory = ConnectionDirectory::make_in(Path::new("/tmp")).unwrap();
    let key = KeyPair::generate("test").await.unwrap();
    let agent = Agent::serve(Signer::new(&key), &directory.path().join("agent")).unwrap();
    let connection = SshConnection {
        master: Mutex::new(Some(
            tokio::process::Command::new("sleep")
                .arg("30")
                .kill_on_drop(true)
                .spawn()
                .unwrap(),
        )),
        keeper: Mutex::new(None),
        dialer: Dialer {
            directory,
            user: "user".into(),
            dial: Dial::Direct {
                endpoint: "127.0.0.1:22".parse().unwrap(),
                host_key: String::new(),
            },
            learnt: std::sync::OnceLock::new(),
        },
        forwarded: None,
        _agent: agent,
        _guard: None,
    };
    let compressed = |on: bool| {
        let command = connection.session_bound_command("true", on).unwrap();
        let arguments: Vec<_> = command
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        arguments.windows(2).any(|w| w == ["-o", "Compression=yes"])
    };
    assert!(compressed(true));
    assert!(!compressed(false));
    // ssh signs in through the connection's agent alone, naming its key by
    // the public half: no private key file is ever handed to it.
    let arguments: Vec<_> = connection
        .session_bound_command("true", false)
        .unwrap()
        .as_std()
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let directory = connection.dialer.directory.path();
    let agent = format!("IdentityAgent={}", directory.join("agent").display());
    let public = directory.join("key.pub").display().to_string();
    assert!(arguments.windows(2).any(|w| w == ["-o", agent.as_str()]));
    assert!(arguments.windows(2).any(|w| w == ["-i", public.as_str()]));
    assert_eq!(arguments.iter().filter(|a| *a == "-i").count(), 1);
    assert!(!arguments
        .iter()
        .any(|a| a.starts_with("IdentityAgent=") && *a != agent));
}

/// Run by [`a_killed_process_leaves_no_key_and_its_directory_is_swept`] in
/// a process of its own: connect to the machine the environment names, say
/// where the connection's directory is, and wait to be killed.
#[tokio::test]
#[ignore = "a helper the kill test runs in its own process"]
async fn kill_helper() {
    let Ok(port) = std::env::var("WSDK_KILL_PORT") else {
        return;
    };
    let host_key = std::env::var("WSDK_KILL_HOST_KEY").unwrap();
    let authorized = std::env::var("WSDK_KILL_AUTHORIZED").unwrap();
    let connection = SshConnection::connect(
        |_| async move {
            let key = KeyPair::generate("killed").await?;
            std::fs::write(&authorized, key.public())?;
            Ok(Route {
                user: "user".into(),
                key,
                dial: Dial::Direct {
                    endpoint: ([127, 0, 0, 1], port.parse().unwrap()).into(),
                    host_key,
                },
                guard: None,
            })
        },
        None,
    )
    .await
    .unwrap();
    println!("directory {}", connection.dialer.directory.path().display());
    tokio::time::sleep(Duration::from_secs(600)).await;
}

/// The files of a connection's directory, and whether any holds a private
/// key.
fn holds_a_private_key(directory: &Path) -> bool {
    std::fs::read_dir(directory)
        .unwrap()
        .flatten()
        .any(|entry| {
            std::fs::read(entry.path())
                .is_ok_and(|bytes| bytes.windows(11).any(|w| w == b"PRIVATE KEY"))
        })
}

#[tokio::test]
#[ignore = "needs a local sshd and an account named user"]
async fn a_killed_process_leaves_no_key_and_its_directory_is_swept() {
    let machine = Machine::start(false).await;
    let mut helper = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "providers::openssh::connection::tests::kill_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("WSDK_KILL_PORT", machine.port.to_string())
        .env("WSDK_KILL_HOST_KEY", &machine.host_key)
        .env(
            "WSDK_KILL_AUTHORIZED",
            machine.place.path().join("authorized"),
        )
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(helper.stdout.take().unwrap()).lines();
    let directory = tokio::time::timeout(Duration::from_secs(30), async {
        while let Some(line) = lines.next_line().await.unwrap() {
            if let Some(path) = line.strip_prefix("directory ") {
                return PathBuf::from(path);
            }
        }
        panic!("the helper did not connect");
    })
    .await
    .unwrap();

    // Connected: the key is in memory only.
    assert!(directory.join("agent").exists());
    assert!(!directory.join("key").exists());
    assert!(!holds_a_private_key(&directory));

    // Killed outright: no destructor runs, and still no key is left.
    helper.start_kill().unwrap();
    helper.wait().await.unwrap();
    assert!(!holds_a_private_key(&directory));
    // Its master outlives it; end it, as the machine or network would.
    let _ = std::process::Command::new("pkill")
        .args(["-KILL", "-f"])
        .arg(directory.join("control"))
        .status();

    // The next process to connect sweeps what is left, once it is old
    // enough not to be one just being made.
    let then = std::time::SystemTime::now() - Duration::from_secs(600);
    std::fs::File::open(&directory)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(then))
        .unwrap();
    super::super::directories::sweep(Path::new("/tmp"), Duration::from_secs(60));
    assert!(!directory.exists());
}
