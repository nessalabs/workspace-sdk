//! A live connection into a Sandbox over OpenSSH. Boat authorizes a fresh
//! key made for this connection and supplies the host's identity; no user's
//! SSH configuration, agent or known hosts are consulted.
//!
//! ```text
//! connect:  ssh-keygen ─► POST sshkey ─► pin host key ─► master ─► -O check
//!           [forward: keeper makes a private directory ─► -O forward -R]
//! stream / run:  a connection each, while the master lives
//! files:          a session through the master
//! drop:     keeper ends (its directory is removed) ─► master ends ─► key removed
//! ```
//!
//! Boat cannot revoke a key: each connection leaves one more authorized key
//! on the machine, whose private half is deleted with the connection. A
//! process that is killed outright (not dropped) also leaves its master
//! running until the machine or the network ends it.
use crate::{
    ports::{
        ConnectionStream, FileChannel, IConnection, IConnectionProcess, ProcessEnd, RunOutput,
        SocketForward, MAX_RUN_OUTPUT,
    },
    providers::managed::quote,
    Error, Result,
};
use async_trait::async_trait;
use base64::Engine;
use sandbox_client::Api;
use std::{
    net::SocketAddr,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStderr, Command};

/// The only account Boat signs keys in as.
const USER: &str = "user";
/// The name the pinned host key is kept under.
const HOST_ALIAS: &str = "workspace-sdk-boat";
const MASTER_START_TIMEOUT: Duration = Duration::from_secs(15);
const KEY_TIMEOUT: Duration = Duration::from_secs(10);
const FORWARD_TIMEOUT: Duration = Duration::from_secs(15);
/// The most of what ssh reports that a diagnostic keeps.
const MAX_DIAGNOSTIC: usize = 512;

/// Runs a command so that it ends when its session does, and leaves nothing
/// behind when it ends. sshd makes the command a process-group leader but
/// sends it nothing when the client goes away; this watches for its session
/// process to end (the shell is then reparented) and ends the whole group,
/// and ends the group's other processes once the command exits. It ends a
/// group only when the command leads it, so it never reaches past its own
/// session. It needs Linux's `/proc`.
const SESSION_BOUND: &str = "p=$PPID; \
    if [ \"$(cut -d \" \" -f 5 /proc/$$/stat 2>/dev/null)\" = \"$$\" ]; then g=0; else g=; fi; \
    (while [ \"$(cut -d \" \" -f 4 /proc/$$/stat 2>/dev/null)\" = \"$p\" ]; do sleep 1; done; \
    [ -n \"$g\" ] && kill -TERM 0) </dev/null >/dev/null 2>&1 & w=$!; \
    sh -c \"$1\"; s=$?; kill $w 2>/dev/null; \
    if [ -n \"$g\" ]; then trap \"\" TERM; kill -TERM 0; fi; exit $s";

/// Makes a private directory, says where, and removes it once its input
/// ends, which is when the connection does.
const KEEPER: &str = "d=$(mktemp -d) || exit 1; echo \"$d\"; cat >/dev/null; rm -rf \"$d\"";

pub(super) struct SshConnection {
    master: Mutex<Option<Child>>,
    /// Holds the forward's private directory; `None` without a forward.
    keeper: Mutex<Option<Child>>,
    directory: tempfile::TempDir,
    endpoint: SocketAddr,
    forwarded: Option<String>,
}

fn connection_error(message: &str) -> Error {
    Error::Provider {
        provider: super::branding::NAME.into(),
        message: format!("connection: {message}"),
    }
}

impl SshConnection {
    /// Connect to `vm`, making `forward` reachable inside it when given.
    pub(super) async fn connect(
        api: &Api,
        vm: &str,
        forward: Option<SocketForward>,
    ) -> Result<Arc<Self>> {
        // A short path: the master's socket must fit a Unix socket address.
        let directory = tempfile::Builder::new()
            .prefix("wsdk-ssh-")
            .tempdir_in("/tmp")?;
        let public_key = make_key(&directory).await?;
        let (endpoint, host_key) = authorize(api, vm, public_key).await?;
        tokio::fs::write(directory.path().join("hosts"), host_key).await?;
        let master = start_master(&directory, endpoint).await?;
        let mut connection = Self {
            master: Mutex::new(Some(master)),
            keeper: Mutex::new(None),
            directory,
            endpoint,
            forwarded: None,
        };
        if let Some(forward) = forward {
            connection.forward(&forward).await?;
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
        let mut request = ssh(&self.directory, self.endpoint);
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

    /// One session, only through the master: with no master it fails
    /// rather than signing in again on its own.
    fn session(&self) -> Command {
        let mut command = ssh(&self.directory, self.endpoint);
        command.args([
            "-o",
            "ControlMaster=no",
            "-o",
            "ClearAllForwardings=yes",
            "-o",
            "ProxyCommand=false",
        ]);
        command
    }
    fn target(&self) -> String {
        format!("{USER}@{}", self.endpoint.ip())
    }
    /// A session running `command` until it, or its session, ends. It has
    /// a connection of its own, so its end is seen on the machine: a session
    /// shared through the master ends without the machine's command being
    /// told. It is refused once the master has ended, as every session is.
    fn bound(&self, command: &str) -> Result<Command> {
        self.require_master()?;
        let mut ssh = ssh_via(&self.directory, self.endpoint, false);
        ssh.arg(self.target())
            .arg(format!(
                "exec sh -c {} wsdk {}",
                quote(SESSION_BOUND),
                quote(command)
            ))
            .stderr(Stdio::piped());
        Ok(ssh)
    }
    /// Fails once the master has ended: its forward is gone with it.
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
        // The keeper first, so its directory goes; then the master, which
        // must be gone before its key and socket files are removed with
        // `directory`.
        for slot in [&mut self.keeper, &mut self.master] {
            if let Some(child) = slot.get_mut().ok().and_then(Option::take) {
                end(child);
            }
        }
    }
}

/// Kill `child` and wait up to 200 ms for it to go.
fn end(mut child: Child) {
    let _ = child.start_kill();
    for _ in 0..40 {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

async fn make_key(directory: &tempfile::TempDir) -> Result<String> {
    let key = directory.path().join("key");
    let mut generate = Command::new("ssh-keygen");
    generate
        .args(["-q", "-t", "ed25519", "-N", "", "-C", HOST_ALIAS, "-f"])
        .arg(&key)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let made = tokio::time::timeout(KEY_TIMEOUT, generate.status())
        .await
        .map_err(|_| connection_error("making a key timed out"))??;
    if !made.success() {
        return Err(connection_error("ssh-keygen failed"));
    }
    Ok(tokio::fs::read_to_string(key.with_extension("pub")).await?)
}

/// Have Boat authorize `public_key` on `vm`; answer where to connect and the
/// `known_hosts` line to pin. Boat must name an IP endpoint and an ed25519
/// host key, or there is nothing to check the machine against.
async fn authorize(api: &Api, vm: &str, public_key: String) -> Result<(SocketAddr, String)> {
    let authorized = api
        .ssh_key(vm, public_key)
        .await
        .map_err(super::error::error)?;
    if !authorized.ok
        || authorized.success != Some(true)
        || authorized.ssh_user.as_deref() != Some(USER)
    {
        return Err(connection_error("the key was not authorized"));
    }
    let endpoint = authorized
        .ssh_endpoint
        .as_deref()
        .and_then(|endpoint| endpoint.parse().ok())
        .ok_or_else(|| connection_error("no usable SSH endpoint"))?;
    let host_key = known_host(authorized.host_key.as_deref())
        .ok_or_else(|| connection_error("no usable host key"))?;
    Ok((endpoint, host_key))
}

async fn start_master(directory: &tempfile::TempDir, endpoint: SocketAddr) -> Result<Child> {
    let target = format!("{USER}@{}", endpoint.ip());
    let mut command = ssh(directory, endpoint);
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
        let mut check = ssh(directory, endpoint);
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

/// One session's process: its child, the end of what ssh reported, and the
/// connection it keeps open.
struct SshProcess {
    child: Child,
    stderr: Option<tokio::task::JoinHandle<String>>,
    _connection: Option<Arc<SshConnection>>,
}
impl SshProcess {
    fn new(mut child: Child, connection: Option<Arc<SshConnection>>) -> Self {
        let stderr = child
            .stderr
            .take()
            .map(|stderr| tokio::spawn(tail_diagnostic(stderr)));
        Self {
            child,
            stderr,
            _connection: connection,
        }
    }
}
#[async_trait]
impl IConnectionProcess for SshProcess {
    async fn wait(&mut self) -> Result<ProcessEnd> {
        let status = self.child.wait().await?;
        // ssh answers 255 for its own failure and for a command killed by a
        // signal; a command's own 255 is read the same way.
        Ok(match status.code() {
            Some(code) if code != 255 => ProcessEnd::Exited(code),
            _ => ProcessEnd::Lost {
                diagnostic: match self.stderr.take() {
                    Some(said) => said.await.unwrap_or_default(),
                    None => String::new(),
                },
            },
        })
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
        let mut ssh = self.bound(command)?;
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
            input.write_all(line).await?;
            input.write_all(b"\n").await?;
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
        let mut ssh = self.bound(command)?;
        ssh.stdin(input.map_or_else(Stdio::null, Stdio::from))
            .stdout(Stdio::piped());
        let mut child = ssh.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| connection_error("no standard output"))?;
        let mut process = SshProcess::new(child, None);
        // On a timeout the session is dropped, which ends the command.
        let (output, end) = tokio::time::timeout(timeout, async {
            let output = read_tail(stdout, MAX_RUN_OUTPUT).await?;
            Ok::<_, Error>((output, process.wait().await?))
        })
        .await
        .map_err(|_| Error::Timeout("the command did not end in time".into()))??;
        match end {
            ProcessEnd::Exited(exit_code) => Ok(RunOutput {
                exit_code,
                output: String::from_utf8_lossy(&output).into_owned(),
            }),
            ProcessEnd::Lost { diagnostic } => Err(connection_error(&format!(
                "the command was lost: {diagnostic}"
            ))),
        }
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

/// The `known_hosts` line for `host_key`, which must be one `ssh-ed25519`
/// key whose bytes are an ed25519 key: never an arbitrary provider line.
fn known_host(host_key: Option<&str>) -> Option<String> {
    let host_key = host_key?;
    if host_key.contains(['\r', '\n']) {
        return None;
    }
    let mut parts = host_key.split_ascii_whitespace();
    let kind = parts.next()?;
    let encoded = parts.next()?;
    if kind != "ssh-ed25519" || encoded.len() > 256 {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    // The string "ssh-ed25519", then the 32 bytes of the key.
    let mut header = Vec::with_capacity(19);
    header.extend_from_slice(&11u32.to_be_bytes());
    header.extend_from_slice(b"ssh-ed25519");
    header.extend_from_slice(&32u32.to_be_bytes());
    if bytes.len() != 51 || bytes[..19] != header[..] {
        return None;
    }
    Some(format!("{HOST_ALIAS} {kind} {encoded}\n"))
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

/// The end of everything a session's ssh reports, read until it closes.
async fn tail_diagnostic(stderr: ChildStderr) -> String {
    printable(&read_tail(stderr, MAX_DIAGNOSTIC).await.unwrap_or_default())
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

fn ssh(directory: &tempfile::TempDir, endpoint: SocketAddr) -> Command {
    ssh_via(directory, endpoint, true)
}

/// ssh to the machine, through the master's control socket when `shared`.
fn ssh_via(directory: &tempfile::TempDir, endpoint: SocketAddr, shared: bool) -> Command {
    let mut command = Command::new("ssh");
    command
        .args([
            "-F",
            "/dev/null",
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "IdentityAgent=none",
            "-o",
            "ForwardAgent=no",
            "-o",
            "ForwardX11=no",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "GlobalKnownHostsFile=/dev/null",
            "-o",
        ])
        .arg(format!("HostKeyAlias={HOST_ALIAS}"))
        .args([
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=2",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "LogLevel=ERROR",
            "-p",
        ])
        .arg(endpoint.port().to_string())
        .arg("-i")
        .arg(directory.path().join("key"))
        .arg("-S")
        .arg(if shared {
            directory.path().join("control")
        } else {
            "none".into()
        })
        .arg("-o")
        .arg(format!(
            "UserKnownHostsFile={}",
            directory.path().join("hosts").display()
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    command
}

#[cfg(test)]
#[path = "connection_tests.rs"]
mod tests;
