//! A live connection into a Sandbox over OpenSSH. Boat authorizes a fresh
//! key made for this connection and supplies the host's identity; no user's
//! SSH configuration, agent or known hosts are consulted.
//!
//! ```text
//! connect:  ssh-keygen ─► POST sshkey ─► pin host key ─► master (-M, -R) ─► -O check
//! stream / run / files:  one ssh each, through the master's socket
//! drop:     the master ends, then its key and socket files are removed
//! ```
use crate::{
    ports::{ConnectionStream, FileChannel, IConnection, SocketForward, MAX_RUN_OUTPUT},
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
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};

/// The account Boat signs the key in as.
const USER: &str = "user";
/// The name the pinned host key is kept under.
const HOST_ALIAS: &str = "workspace-sdk-boat";
/// How long the master may take to come up.
const MASTER_START: Duration = Duration::from_secs(15);
/// How long making a key may take.
const KEY_START: Duration = Duration::from_secs(10);

pub(super) struct SshConnection {
    // The master ends before its private files are removed.
    _master: Mutex<Child>,
    directory: tempfile::TempDir,
    endpoint: SocketAddr,
    forwarded: Option<String>,
}

fn failed(message: &str) -> Error {
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
        let key = directory.path().join("key");
        let mut generate = Command::new("ssh-keygen");
        generate
            .args(["-q", "-t", "ed25519", "-N", "", "-C", HOST_ALIAS, "-f"])
            .arg(&key)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let made = tokio::time::timeout(KEY_START, generate.status())
            .await
            .map_err(|_| failed("making a key timed out"))??;
        if !made.success() {
            return Err(failed("ssh-keygen failed"));
        }
        let public_key = tokio::fs::read_to_string(key.with_extension("pub")).await?;
        let authorized = api
            .ssh_key(vm, public_key)
            .await
            .map_err(super::error::error)?;
        if !authorized.ok
            || authorized.success != Some(true)
            || authorized.ssh_user.as_deref() != Some(USER)
        {
            return Err(failed("the key was not authorized"));
        }
        let endpoint: SocketAddr = authorized
            .ssh_endpoint
            .as_deref()
            .and_then(|endpoint| endpoint.parse().ok())
            .ok_or_else(|| failed("no usable SSH endpoint"))?;
        let host_key = known_host(authorized.host_key.as_deref())
            .ok_or_else(|| failed("no usable host key"))?;
        tokio::fs::write(directory.path().join("hosts"), host_key).await?;
        let forwarded = forward.as_ref().map(|_| {
            format!(
                "/tmp/workspace-sdk-forward-{}.sock",
                uuid::Uuid::new_v4().simple()
            )
        });
        let mut command = ssh(&directory, endpoint);
        command.args([
            "-M",
            "-N",
            "-o",
            "ControlPersist=no",
            "-o",
            "ExitOnForwardFailure=yes",
        ]);
        if let (Some(forward), Some(remote)) = (&forward, &forwarded) {
            command
                .arg("-R")
                .arg(format!("{remote}:{}", forward.local.display()));
        }
        command
            .arg(format!("{USER}@{}", endpoint.ip()))
            .stderr(Stdio::piped());
        let mut master = command.spawn()?;
        let mut diagnostics = master.stderr.take();
        let deadline = tokio::time::Instant::now() + MASTER_START;
        loop {
            let problem = if master.try_wait()?.is_some() {
                "the SSH connection ended while starting"
            } else if tokio::time::Instant::now() >= deadline {
                "the SSH connection did not start in time"
            } else {
                ""
            };
            if !problem.is_empty() {
                let _ = master.start_kill();
                let said = match diagnostics.take() {
                    Some(stderr) => said(stderr).await,
                    None => String::new(),
                };
                return Err(failed(&format!("{problem}: {said}")));
            }
            let mut check = ssh(&directory, endpoint);
            check
                .args(["-O", "check"])
                .arg(format!("{USER}@{}", endpoint.ip()));
            if tokio::time::timeout(Duration::from_secs(1), check.status())
                .await
                .is_ok_and(|status| status.is_ok_and(|status| status.success()))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        // Whatever it says later is read and dropped, so it never blocks.
        if let Some(mut stderr) = diagnostics {
            tokio::spawn(async move {
                let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
            });
        }
        Ok(Arc::new(Self {
            _master: Mutex::new(master),
            directory,
            endpoint,
            forwarded,
        }))
    }

    /// One ssh through the master, without forwards of its own.
    fn session(&self) -> Command {
        let mut command = ssh(&self.directory, self.endpoint);
        command.args(["-o", "ControlMaster=no", "-o", "ClearAllForwardings=yes"]);
        command
    }
    fn target(&self) -> String {
        format!("{USER}@{}", self.endpoint.ip())
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
        let mut ssh = self.session();
        ssh.arg(self.target())
            .arg(posix(command))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut child = ssh.spawn()?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| failed("no standard output"))?;
        let mut input = child
            .stdin
            .take()
            .ok_or_else(|| failed("no standard input"))?;
        if let Some(line) = first_line {
            input.write_all(line).await?;
            input.write_all(b"\n").await?;
        }
        Ok(ConnectionStream {
            input: Box::new(input),
            output: Box::new(output),
            keep: Box::new((child, self)),
        })
    }

    async fn run(&self, command: &str, input: Option<std::fs::File>) -> Result<String> {
        let mut ssh = self.session();
        // What is sent is compressed on the way.
        if input.is_some() {
            ssh.args(["-o", "Compression=yes"]);
        }
        ssh.arg(self.target())
            .arg(posix(command))
            .stdin(input.map_or_else(Stdio::null, Stdio::from))
            .stdout(Stdio::piped());
        let mut child = ssh.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| failed("no standard output"))?;
        let output = read_tail(stdout, MAX_RUN_OUTPUT).await?;
        child.wait().await?;
        Ok(String::from_utf8_lossy(&output).into_owned())
    }

    async fn files(self: Arc<Self>) -> Result<FileChannel> {
        let mut ssh = self.session();
        ssh.arg("-s")
            .arg(self.target())
            .arg("sftp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut child = ssh.spawn()?;
        let from_machine = child
            .stdout
            .take()
            .ok_or_else(|| failed("no SFTP output"))?;
        let to_machine = child.stdin.take().ok_or_else(|| failed("no SFTP input"))?;
        Ok(FileChannel {
            from_machine: Box::new(from_machine),
            to_machine: Box::new(to_machine),
            keep: Box::new((child, self)),
        })
    }

    fn forwarded_socket(&self) -> Option<&str> {
        self.forwarded.as_deref()
    }
}

/// The `known_hosts` line for `host_key`, which must be one plain
/// `ssh-ed25519` key: never an arbitrary line from the provider.
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
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    Some(format!("{HOST_ALIAS} {kind} {encoded}\n"))
}

/// What ssh said as it failed: at most 512 bytes, read for at most a
/// second, printable ASCII only, so no provider text can forge a line.
async fn said(stderr: tokio::process::ChildStderr) -> String {
    let mut bytes = Vec::new();
    let _ = tokio::time::timeout(
        Duration::from_secs(1),
        stderr.take(512).read_to_end(&mut bytes),
    )
    .await;
    String::from_utf8_lossy(&bytes)
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

/// `script` run by `sh`, whatever the account's login shell is.
fn posix(script: &str) -> String {
    format!("sh -c '{}'", script.replace('\'', "'\\''"))
}

/// The last `limit` bytes `reader` gives before it ends.
async fn read_tail(
    mut reader: impl tokio::io::AsyncRead + Unpin,
    limit: usize,
) -> std::io::Result<Vec<u8>> {
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
        .arg(directory.path().join("c"))
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
