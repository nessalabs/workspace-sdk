//! A live connection into a provider's machine over OpenSSH. The provider
//! authorizes a key for it and says how to reach the machine ([`Route`]): a
//! TCP endpoint whose host key it pins, or a tunnel it serves on a local Unix
//! socket. No user's SSH configuration, agent or known hosts are consulted.
//!
//! ```text
//! connect:  authorize (key, route) ─► master ─► -O check
//!           [forward: keeper makes a private directory ─► -O forward -R]
//! stream / run:  a connection each, while the master lives, under a
//!                 wrapper that ends the command's session with it
//! files:          a session through the master
//! drop:     keeper ends (its directory is removed) ─► master ends
//!           ─► directory removed ─► agent and tunnel end
//! ```
//!
//! The connection never writes the private key to disk (nor does making
//! one, [`KeyPair::generate`]): ssh signs in through an agent
//! this process serves on a socket in the connection's directory
//! ([`super::agent`]). A process killed outright leaves that directory (its
//! socket, public key and learnt host key) behind; the next process to
//! connect sweeps it ([`super::directories`]).
//!
//! Through a tunnel the machine's host key cannot be known beforehand: the
//! master learns it, and every later session of the connection must match
//! it. The tunnel's own authentication (TLS and the provider's credential)
//! is what identifies the machine. A process that is killed outright (not
//! dropped) also leaves its master running until the machine or the
//! network ends it. A client cut off by a network partition is seen only
//! when sshd notices, which may take hours with its default settings.
//! Endpoints are taken by address: an IPv6 address's scope is not kept.
use super::{
    agent::{Agent, Signer},
    directories::ConnectionDirectory,
    KeyPair,
};
use crate::{
    ports::{
        ConnectionStream, FileChannel, IConnection, IConnectionProcess, ProcessEnd, ProcessExit,
        RunOutput, SocketForward, MAX_RUN_OUTPUT, MAX_STDERR,
    },
    providers::managed::quote,
    Error, Result,
};
use async_trait::async_trait;
use std::{
    future::Future,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStderr, Command};

/// The name the machine's host key is kept under.
pub(in crate::providers) const HOST_ALIAS: &str = "workspace-sdk-machine";
const MASTER_START_TIMEOUT: Duration = Duration::from_secs(15);
const FORWARD_TIMEOUT: Duration = Duration::from_secs(15);
const FIRST_LINE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long the first command through a new connection may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
/// The most of what ssh reports that a diagnostic keeps.
const MAX_DIAGNOSTIC: usize = 512;

/// Ends every process of the session `$me` but `$skip`: asks them, waits up
/// to five seconds, then kills what is left. A process that started a
/// session of its own is not reached. The grace needs a `sleep` that takes
/// fractions, as GNU's and busybox's do; and a process that ends and whose
/// number is reused within microseconds could be signalled in its place,
/// which a shell cannot rule out.
macro_rules! end_session {
    () => {
        r#"k() {
  f=0
  for d in /proc/[0-9]*; do
    n=${d#/proc/}
    [ "$n" = "$skip" ] && continue
    read -r l 2>/dev/null < "$d/stat" || continue
    set -- ${l##*) }
    [ "$4" = "$me" ] && [ "$1" != Z ] || continue
    f=1
    kill -"$sig" "$n" 2>/dev/null
  done
}
finish() {
  sig=TERM; k; i=0
  while [ "$f" = 1 ] && [ "$i" -lt 50 ]; do sleep 0.1; sig=0; k; i=$((i + 1)); done
  sig=KILL; k
}
"#
    };
}

/// Watches the session `$1`, whose leader's parent is `$2` (sshd's process
/// for this connection), and ends the session once that parent is gone:
/// sshd tells a command nothing when its client goes away. It needs Linux's
/// `/proc`, and polls once a second.
const WATCH: &str = concat!(
    end_session!(),
    r#"trap '' TERM
me=$1; p=$2; skip=$$
while read -r l 2>/dev/null < "/proc/$me/stat"; do
  set -- ${l##*) }
  [ "$2" = "$p" ] || break
  sleep 1
done
finish
"#
);

/// Runs the command `$1` (with the watcher `$2`) so that it ends when its
/// connection does, and leaves nothing of its session behind when it exits.
/// It refuses to run unless it leads its session and its parent is sshd, as
/// sshd makes it, so it never reaches past what this connection started and
/// never runs once that connection has already gone.
const SESSION_BOUND: &str = concat!(
    end_session!(),
    r#"c=$1; watch=$2; me=$$
read -r l 2>/dev/null < "/proc/$me/stat" || { echo "workspace-sdk: the machine has no /proc" >&2; exit 255; }
set -- ${l##*) }; p=$2
[ "$4" = "$me" ] || { echo "workspace-sdk: the command does not lead its session" >&2; exit 255; }
read -r n 2>/dev/null < "/proc/$p/comm"
case $n in sshd*) ;; *) echo "workspace-sdk: the command's session already ended" >&2; exit 255;; esac
sh -c "$watch" workspace-sdk-watch "$me" "$p" < /dev/null > /dev/null 2>&1 &
w=$!
sh -c -- "$c"; s=$?
kill -KILL "$w" 2>/dev/null
trap '' TERM; skip=$me; finish
exit "$s"
"#
);

/// Makes a private directory, says where, and removes it once its input
/// ends, which is when the connection does.
const KEEPER: &str = "d=$(mktemp -d) || exit 1; echo \"$d\"; cat >/dev/null; rm -rf \"$d\"";

/// How a connection reaches its machine, as its provider authorized it.
/// The connection keeps the key in memory, and writes its public half and
/// any pinned host key into its own private directory.
pub(in crate::providers) struct Route {
    /// The account the key signs in as.
    pub user: String,
    /// The key the machine accepts for `user`.
    pub key: KeyPair,
    /// Where ssh dials.
    pub dial: Dial,
    /// Whatever must live as long as the connection, such as the tunnel it
    /// dials through. Dropped after the master.
    pub guard: Option<Box<dyn Send + Sync>>,
}
/// Where a connection's ssh dials.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::providers) enum Dial {
    /// A TCP endpoint whose host key is pinned: an ed25519 key, as
    /// [`super::ed25519_key`] accepts it.
    Direct {
        /// The machine's SSH endpoint.
        endpoint: SocketAddr,
        /// The machine's host key.
        host_key: String,
    },
    /// Every connection through `socket`, a local Unix socket the provider
    /// serves that carries it to the machine named `host`. The first
    /// connection learns the host key; every later one must match it.
    #[allow(dead_code)] // Only providers with a tunnel make it.
    Tunnel {
        /// The provider's local socket, inside the connection directory.
        socket: PathBuf,
        /// The machine's name, for ssh's messages only.
        host: String,
    },
}

/// The ssh command lines of one connection.
struct Dialer {
    directory: ConnectionDirectory,
    user: String,
    dial: Dial,
    /// Through a tunnel, the host key algorithms of the key the master
    /// learnt: from then on, every session must present that key.
    learnt: std::sync::OnceLock<String>,
}

pub(in crate::providers) struct SshConnection {
    master: Mutex<Option<Child>>,
    /// Holds the forward's private directory; `None` without a forward.
    keeper: Mutex<Option<Child>>,
    dialer: Dialer,
    forwarded: Option<String>,
    /// Signs in for every ssh of this connection; it outlives the master.
    _agent: Agent,
    /// The route's own state, such as a tunnel; it outlives the master.
    _guard: Option<Box<dyn Send + Sync>>,
}

fn connection_error(message: &str) -> Error {
    Error::Provider {
        provider: "ssh".into(),
        message: format!("connection: {message}"),
    }
}

impl SshConnection {
    /// Connect to a machine, making `forward` reachable inside it when given.
    ///
    /// `authorize` gets the connection's private directory, where a tunnel
    /// it serves puts its socket, and answers the route. Everything in the
    /// directory is removed with the connection, or as soon as connecting
    /// fails.
    pub(in crate::providers) async fn connect<F, Fut>(
        authorize: F,
        forward: Option<SocketForward>,
    ) -> Result<Arc<Self>>
    where
        F: FnOnce(PathBuf) -> Fut,
        Fut: Future<Output = Result<Route>>,
    {
        let directory = ConnectionDirectory::make()?;
        let route = authorize(directory.path().to_owned()).await?;
        let hosts = match &route.dial {
            Dial::Direct { host_key, .. } => {
                let key = super::ed25519_key(host_key)
                    .ok_or_else(|| connection_error("no usable host key"))?;
                format!("{HOST_ALIAS} {key}\n")
            }
            Dial::Tunnel { socket, .. } => {
                if socket.parent() != Some(directory.path()) || !plain_path(socket) {
                    return Err(connection_error("a tunnel's socket is not its own"));
                }
                // Learnt by the master, then held to.
                String::new()
            }
        };
        tokio::fs::write(directory.path().join("hosts"), hosts).await?;
        let signer = Signer::new(&route.key);
        tokio::fs::write(
            directory.path().join("key.pub"),
            format!("{}\n", route.key.public()),
        )
        .await?;
        let agent = Agent::serve(signer, &directory.path().join("agent"))?;
        if route.user.is_empty()
            || !route
                .user
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(connection_error("the account name is unusable"));
        }
        let dialer = Dialer {
            directory,
            user: route.user,
            dial: route.dial,
            learnt: std::sync::OnceLock::new(),
        };
        // A master dropped on any failure below is killed with it.
        let master = dialer.start_master().await?;
        if matches!(dialer.dial, Dial::Tunnel { .. }) {
            dialer.pin().await?;
        }
        let mut connection = Self {
            master: Mutex::new(Some(master)),
            keeper: Mutex::new(None),
            dialer,
            forwarded: None,
            _agent: agent,
            _guard: route.guard,
        };
        if let Some(forward) = forward {
            connection.forward(&forward).await?;
        }
        // A machine that cannot bind a command is refused here, so a command
        // reported lost later may really have run.
        // A login that never reaches the wrapper fails within the deadline,
        // and dropping the connection releases the master, key and forward.
        let mut probe = connection.session_bound_command("true", false)?;
        probe.stdout(Stdio::null()).kill_on_drop(true);
        let probe = tokio::time::timeout(PROBE_TIMEOUT, probe.status())
            .await
            .map_err(|_| connection_error("the machine did not run a first command in time"))??;
        if !probe.success() {
            return Err(connection_error(
                "the machine cannot end a command with its connection",
            ));
        }
        Ok(Arc::new(connection))
    }

    /// Bind `forward` in a private directory on the machine, and answer once
    /// the machine has bound it.
    async fn forward(&mut self, forward: &SocketForward) -> Result<()> {
        let mut keeper = self.session();
        keeper
            .arg(self.target())
            .arg(KEEPER)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut keeper = keeper.spawn()?;
        let mut stdout = keeper
            .stdout
            .take()
            .ok_or_else(|| connection_error("no keeper output"))?;
        *self
            .keeper
            .get_mut()
            .map_err(|_| connection_error("poisoned"))? = Some(keeper);
        let line = tokio::time::timeout(FORWARD_TIMEOUT, read_line(&mut stdout))
            .await
            .map_err(|_| connection_error("the forward's directory was not made in time"))??;
        if !line.starts_with('/') || line.contains(':') || line.chars().any(char::is_control) {
            return Err(connection_error("the forward's directory is unusable"));
        }
        let remote = format!("{line}/forward.sock");
        let mut request = self.dialer.ssh(true, false);
        request
            .args(["-O", "forward", "-R"])
            .arg(format!("{remote}:{}", forward.local().display()))
            .arg(self.target())
            .stderr(Stdio::piped());
        let mut request = request.spawn()?;
        let stderr = request.stderr.take();
        let answered = tokio::time::timeout(FORWARD_TIMEOUT, request.wait())
            .await
            .map_err(|_| connection_error("the forward was not answered in time"))??;
        if !answered.success() {
            let said = match stderr {
                Some(stderr) => diagnostic(stderr).await,
                None => String::new(),
            };
            return Err(connection_error(&format!(
                "the forward was refused: {said}"
            )));
        }
        self.forwarded = Some(remote);
        Ok(())
    }

    /// One session through the master. With the master gone it fails rather
    /// than opening a connection of its own, which would hide that the
    /// connection, and its forward, were lost.
    fn session(&self) -> Command {
        let mut command = self.dialer.ssh(true, false);
        command.args(["-o", "ControlMaster=no", "-o", "ClearAllForwardings=yes"]);
        command
    }
    fn target(&self) -> String {
        self.dialer.target()
    }
    /// A session running `command` until it, or its session, ends. It has
    /// a connection of its own, so its end is seen on the machine: a session
    /// shared through the master ends without the machine's command being
    /// told. It is refused once the master has ended, as every session is.
    fn session_bound_command(&self, command: &str, compressed: bool) -> Result<Command> {
        self.require_master()?;
        let mut ssh = self.dialer.ssh(false, true);
        if compressed {
            ssh.args(["-o", "Compression=yes"]);
        }
        ssh.arg(self.target())
            .arg(format!(
                "exec sh -c {} workspace-sdk {} {}",
                quote(SESSION_BOUND),
                quote(command),
                quote(WATCH)
            ))
            .stderr(Stdio::piped());
        Ok(ssh)
    }
    /// Whether its master still lives, so new commands may start.
    #[allow(dead_code)] // Only providers that reuse a connection ask.
    pub(in crate::providers) fn alive(&self) -> bool {
        self.require_master().is_ok()
    }
    /// Fails once the master has ended: its forward is gone with it. A
    /// command already running is not told.
    fn require_master(&self) -> Result<()> {
        let mut master = self
            .master
            .lock()
            .map_err(|_| connection_error("poisoned"))?;
        match master.as_mut().map(Child::try_wait) {
            Some(Ok(None)) => Ok(()),
            _ => Err(connection_error(
                "the connection was lost: its master ended",
            )),
        }
    }
}

impl Drop for SshConnection {
    fn drop(&mut self) {
        // The keeper first, so its directory goes; then the master. Its
        // socket files may go with `directory` while it is still ending:
        // nothing uses them once it is dead.
        for slot in [&mut self.keeper, &mut self.master] {
            if let Some(child) = slot.get_mut().ok().and_then(Option::take) {
                kill_and_reap(child);
            }
        }
    }
}

/// Kill `child` now, and reap it without blocking the caller's thread.
fn kill_and_reap(mut child: Child) {
    let _ = child.start_kill();
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        runtime.spawn(async move {
            let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
        });
    } else {
        let _ = child.try_wait();
    }
}

impl Dialer {
    /// Hold every later session to the one host key the master learnt.
    async fn pin(&self) -> Result<()> {
        let hosts = tokio::fs::read_to_string(self.directory.path().join("hosts")).await?;
        let mut lines = hosts.lines().filter(|line| !line.trim().is_empty());
        let (Some(line), None) = (lines.next(), lines.next()) else {
            return Err(connection_error("the machine did not present one host key"));
        };
        let algorithms = match line.split_ascii_whitespace().nth(1) {
            Some(
                kind @ ("ssh-ed25519"
                | "ecdsa-sha2-nistp256"
                | "ecdsa-sha2-nistp384"
                | "ecdsa-sha2-nistp521"),
            ) => kind.to_owned(),
            Some("ssh-rsa") => "rsa-sha2-512,rsa-sha2-256".to_owned(),
            _ => {
                return Err(connection_error(
                    "the machine's host key is of no known kind",
                ))
            }
        };
        let _ = self.learnt.set(algorithms);
        Ok(())
    }

    async fn start_master(&self) -> Result<Child> {
        let target = self.target();
        let mut command = self.ssh(true, true);
        command
            .args(["-M", "-N", "-o", "ControlPersist=no"])
            .arg(&target)
            .stderr(Stdio::piped());
        let mut master = command.spawn()?;
        let mut stderr = master.stderr.take();
        let deadline = tokio::time::Instant::now() + MASTER_START_TIMEOUT;
        loop {
            let problem = if master.try_wait()?.is_some() {
                Some("the SSH connection ended while starting")
            } else if tokio::time::Instant::now() >= deadline {
                Some("the SSH connection did not start in time")
            } else {
                None
            };
            if let Some(problem) = problem {
                let _ = master.start_kill();
                let said = match stderr.take() {
                    Some(stderr) => diagnostic(stderr).await,
                    None => String::new(),
                };
                return Err(connection_error(&format!("{problem}: {said}")));
            }
            let mut check = self.ssh(true, false);
            check.args(["-O", "check"]).arg(&target);
            if tokio::time::timeout(Duration::from_secs(1), check.status())
                .await
                .is_ok_and(|status| status.is_ok_and(|status| status.success()))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        // Whatever it says later is read and dropped, so it never blocks.
        if let Some(mut stderr) = stderr {
            tokio::spawn(async move {
                let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
            });
        }
        Ok(master)
    }

    fn target(&self) -> String {
        match &self.dial {
            Dial::Direct { endpoint, .. } => format!("{}@{}", self.user, endpoint.ip()),
            Dial::Tunnel { host, .. } => format!("{}@{host}", self.user),
        }
    }

    /// ssh to the machine, through the master's control socket when
    /// `shared`. Unless `dial`, it never opens a connection of its own: one
    /// that finds the master gone fails rather than hide that the
    /// connection, and its forward, were lost.
    fn ssh(&self, shared: bool, dial: bool) -> Command {
        let directory = self.directory.path();
        let mut command = Command::new("ssh");
        // ssh keeps the first value it is given for an option, so refusing
        // to dial comes before any way to dial.
        if !dial {
            command.args(["-o", "ProxyCommand=false"]);
        }
        command.args([
            "-F",
            "/dev/null",
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "ForwardAgent=no",
            "-o",
            "ForwardX11=no",
            "-o",
            "GlobalKnownHostsFile=/dev/null",
            "-o",
        ]);
        command.arg(format!("HostKeyAlias={HOST_ALIAS}"));
        match &self.dial {
            Dial::Direct { endpoint, .. } => {
                command
                    .args(["-o", "StrictHostKeyChecking=yes", "-p"])
                    .arg(endpoint.port().to_string());
            }
            Dial::Tunnel { socket, .. } => {
                // The first connection, the master's, learns the key into
                // this connection's own file; every later one must present
                // that key, of that kind: no key of another kind is learnt
                // beside it.
                match self.learnt.get() {
                    Some(algorithms) => command
                        .args(["-o", "StrictHostKeyChecking=yes", "-o"])
                        .arg(format!("HostKeyAlgorithms={algorithms}")),
                    None => command.args(["-o", "StrictHostKeyChecking=accept-new"]),
                };
                command.args(["-o", "CheckHostIP=no"]);
                if dial {
                    // ssh runs it as `exec <command>` under the user's
                    // shell; a second `exec` fails under bash.
                    command
                        .arg("-o")
                        .arg(format!("ProxyCommand=nc -U {}", socket.display()));
                }
            }
        }
        command
            .args([
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=2",
                "-o",
                "ConnectTimeout=10",
                "-o",
                "LogLevel=ERROR",
                "-o",
            ])
            // Only this connection's agent, and only its one key: the
            // public half names it, and the agent signs with it.
            .arg(format!(
                "IdentityAgent={}",
                directory.join("agent").display()
            ))
            .arg("-i")
            .arg(directory.join("key.pub"))
            .arg("-S")
            .arg(if shared {
                directory.join("control")
            } else {
                "none".into()
            })
            .arg("-o")
            .arg(format!(
                "UserKnownHostsFile={}",
                directory.join("hosts").display()
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        command
    }
}

/// A path ssh can carry in an option unquoted: printable ASCII with no
/// space, quote or shell metacharacter.
fn plain_path(path: &Path) -> bool {
    path.to_str().is_some_and(|path| {
        path.len() <= crate::ports::MAX_SOCKET_PATH
            && path.starts_with('/')
            && path
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c))
    })
}

/// One session's process: its child, the end of its standard error, and the
/// connection it keeps open.
struct SshProcess {
    child: Child,
    stderr: Option<tokio::task::JoinHandle<String>>,
    _connection: Option<Arc<SshConnection>>,
}
impl SshProcess {
    fn new(mut child: Child, connection: Option<Arc<SshConnection>>) -> Self {
        let stderr = child.stderr.take().map(|stderr| {
            tokio::spawn(async move {
                String::from_utf8_lossy(&read_tail(stderr, MAX_STDERR).await.unwrap_or_default())
                    .into_owned()
            })
        });
        Self {
            child,
            stderr,
            _connection: connection,
        }
    }
}
#[async_trait]
impl IConnectionProcess for SshProcess {
    async fn wait(&mut self) -> Result<ProcessExit> {
        let status = self.child.wait().await?;
        // ssh answers 255 for its own failure, and the wrapper for a
        // machine it cannot bind a command on; a command's own 255 reads
        // the same way.
        let end = match status.code() {
            Some(code) if code != 255 => ProcessEnd::Exited(code),
            _ => ProcessEnd::Lost,
        };
        let stderr = match self.stderr.take() {
            Some(stderr) => tokio::time::timeout(Duration::from_secs(1), stderr)
                .await
                .ok()
                .and_then(std::result::Result::ok)
                .unwrap_or_default(),
            None => String::new(),
        };
        Ok(ProcessExit { end, stderr })
    }
}

#[async_trait]
impl IConnection for SshConnection {
    async fn stream(
        self: Arc<Self>,
        command: &str,
        first_line: Option<&[u8]>,
    ) -> Result<ConnectionStream> {
        if first_line.is_some_and(|line| line.contains(&b'\n')) {
            return Err(Error::InvalidInput("a first line holds no newline".into()));
        }
        let mut ssh = self.session_bound_command(command, false)?;
        ssh.stdin(Stdio::piped()).stdout(Stdio::piped());
        let mut child = ssh.spawn()?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| connection_error("no standard output"))?;
        let mut input = child
            .stdin
            .take()
            .ok_or_else(|| connection_error("no standard input"))?;
        if let Some(line) = first_line {
            tokio::time::timeout(FIRST_LINE_TIMEOUT, async {
                input.write_all(line).await?;
                input.write_all(b"\n").await
            })
            .await
            .map_err(|_| connection_error("the first line was not taken in time"))??;
        }
        Ok(ConnectionStream {
            input: Box::new(input),
            output: Box::new(output),
            process: Box::new(SshProcess::new(child, Some(self))),
        })
    }

    async fn run(
        &self,
        command: &str,
        input: Option<std::fs::File>,
        timeout: Duration,
    ) -> Result<RunOutput> {
        // A file sent, such as a build, is compressed on the way.
        let mut ssh = self.session_bound_command(command, input.is_some())?;
        ssh.stdin(input.map_or_else(Stdio::null, Stdio::from))
            .stdout(Stdio::piped());
        let mut child = ssh.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| connection_error("no standard output"))?;
        let mut process = SshProcess::new(child, None);
        // On a timeout the session is dropped, which ends the command.
        let (output, exit) = tokio::time::timeout(timeout, async {
            let output = read_tail(stdout, MAX_RUN_OUTPUT).await?;
            Ok::<_, Error>((output, process.wait().await?))
        })
        .await
        .map_err(|_| Error::Timeout("the command did not end in time".into()))??;
        Ok(RunOutput {
            end: exit.end,
            output: String::from_utf8_lossy(&output).into_owned(),
            stderr: exit.stderr,
        })
    }

    async fn files(self: Arc<Self>) -> Result<FileChannel> {
        self.require_master()?;
        let mut ssh = self.session();
        ssh.arg("-s")
            .arg(self.target())
            .arg("sftp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = ssh.spawn()?;
        let from_machine = child
            .stdout
            .take()
            .ok_or_else(|| connection_error("no SFTP output"))?;
        let to_machine = child
            .stdin
            .take()
            .ok_or_else(|| connection_error("no SFTP input"))?;
        Ok(FileChannel {
            from_machine: Box::new(from_machine),
            to_machine: Box::new(to_machine),
            process: Box::new(SshProcess::new(child, Some(self))),
        })
    }

    fn forwarded_socket(&self) -> Option<&str> {
        self.forwarded.as_deref()
    }
}

/// What ssh said as it failed: at most [`MAX_DIAGNOSTIC`] bytes, read for at
/// most a second.
async fn diagnostic(stderr: ChildStderr) -> String {
    let mut bytes = Vec::new();
    let _ = tokio::time::timeout(
        Duration::from_secs(1),
        stderr.take(MAX_DIAGNOSTIC as u64).read_to_end(&mut bytes),
    )
    .await;
    printable(&bytes)
}

/// Printable ASCII only, so no provider text can forge a line.
fn printable(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() || c == ' ' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// One line of at most 4 KiB, without its newline.
async fn read_line(reader: &mut (impl AsyncRead + Unpin)) -> Result<String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if reader.read(&mut byte).await? == 0 || line.len() > 4096 {
            return Err(connection_error("no line was read"));
        }
        if byte[0] == b'\n' {
            return String::from_utf8(line).map_err(|_| connection_error("a line was not UTF-8"));
        }
        line.push(byte[0]);
    }
}

/// The last `limit` bytes `reader` gives before it ends.
async fn read_tail(mut reader: impl AsyncRead + Unpin, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut kept = std::collections::VecDeque::with_capacity(limit.min(8192));
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            return Ok(kept.into());
        }
        kept.extend(&buffer[..read]);
        let over = kept.len().saturating_sub(limit);
        kept.drain(..over);
    }
}

#[cfg(test)]
#[path = "connection_tests.rs"]
mod tests;
