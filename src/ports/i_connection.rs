//! A live connection into a workspace's machine: long-running streams, short
//! commands, a file channel, and one local socket reachable from inside.
//!
//! [`IBackend::exec`](super::IBackend::exec) and the managed runs are the
//! SDK's own supervised processes, relative to the project and recorded by
//! the SDK. A connection is for what an application runs itself and keeps
//! its own protocol for, such as an agent that talks over standard input
//! and output for hours, or a check of what the machine's image holds.
use crate::{Error, Result};
use async_trait::async_trait;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite};

/// Bytes into a connection, such as a process's standard input.
pub type ConnectionInput = Box<dyn AsyncWrite + Send + Unpin>;
/// Bytes out of a connection, such as a process's standard output.
pub type ConnectionOutput = Box<dyn AsyncRead + Send + Unpin>;

/// The longest local socket path a forward takes, in bytes: it must fit a
/// Unix socket address on every platform.
pub const MAX_SOCKET_PATH: usize = 100;

/// One local socket a connection makes reachable inside the machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocketForward {
    local: PathBuf,
}
impl SocketForward {
    /// Forward the listening Unix socket at `local`.
    ///
    /// # Errors
    /// [`Error::InvalidInput`] unless `local` is an absolute UTF-8 path of
    /// at most [`MAX_SOCKET_PATH`] bytes without `:` or control characters,
    /// which a forward request would read as something else.
    pub fn new(local: impl Into<PathBuf>) -> Result<Self> {
        let local = local.into();
        let plain = local.to_str().is_some_and(|path| {
            path.len() <= MAX_SOCKET_PATH
                && !path.contains(':')
                && !path.chars().any(char::is_control)
        });
        if !plain || !local.is_absolute() {
            return Err(Error::InvalidInput(
                "a forwarded socket is an absolute, short, plain path".into(),
            ));
        }
        Ok(Self { local })
    }
    /// The local socket.
    pub fn local(&self) -> &Path {
        &self.local
    }
}

/// How a process on the machine ended, as far as the connection can tell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessEnd {
    /// It exited with this status.
    Exited(i32),
    /// The connection ended first, or the process was killed by a signal:
    /// the two look the same from here. `diagnostic` is the end of what the
    /// connection reported, printable ASCII only.
    Lost { diagnostic: String },
}

/// A process started through a connection. Dropping it ends the process
/// within a few seconds, with every process it started that stayed in its
/// process group; those also end when it exits.
#[async_trait]
pub trait IConnectionProcess: Send {
    /// Wait for it to end.
    async fn wait(&mut self) -> Result<ProcessEnd>;
}

/// A process started by [`IConnection::stream`].
pub struct ConnectionStream {
    /// Its standard input, after the first line. Closing it tells the
    /// process its input ended; it does not stop it.
    pub input: ConnectionInput,
    /// Its standard output.
    pub output: ConnectionOutput,
    /// The process itself; dropping it ends the process.
    pub process: Box<dyn IConnectionProcess>,
}

/// The two ends of a file channel; see [`IConnection::files`].
pub struct FileChannel {
    /// What the machine's file server sends.
    pub from_machine: ConnectionOutput,
    /// What is sent to the machine's file server.
    pub to_machine: ConnectionInput,
    /// The file server; dropping it closes the channel.
    pub process: Box<dyn IConnectionProcess>,
}

/// What [`IConnection::run`] answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunOutput {
    /// The command's exit status.
    pub exit_code: i32,
    /// The last [`MAX_RUN_OUTPUT`] bytes of its standard output, decoded
    /// lossily.
    pub output: String,
}

/// The most of a [`IConnection::run`] command's output that is kept: its
/// last bytes, so a failure's end is never lost.
pub const MAX_RUN_OUTPUT: usize = 64 * 1024;

/// A connection into one workspace's machine. It stays open while any
/// handle to it, or any stream or file channel from it, is held.
///
/// Every `command` is a POSIX `sh` script, run in the machine account's home
/// directory; the caller changes directory itself. Nothing an application
/// passes in a command line is secret: a secret goes in a stream's first
/// line.
#[async_trait]
pub trait IConnection: Send + Sync {
    /// Start `command`, writing `first_line` and a newline to its standard
    /// input before anything else, so a secret the command reads first never
    /// appears in a command line or on a disk. `first_line` holds no
    /// newline. The stream keeps the connection open.
    async fn stream(
        self: Arc<Self>,
        command: &str,
        first_line: Option<&[u8]>,
    ) -> Result<ConnectionStream>;

    /// Run `command` with the file `input` on its standard input, and wait
    /// at most `timeout` for it to exit. Processes it leaves behind in its
    /// process group end with it.
    ///
    /// # Errors
    /// [`Error::Timeout`] when `timeout` passed, the command then being
    /// ended as a dropped process is; [`Error::Provider`] when the
    /// connection failed or the command was killed by a signal.
    async fn run(
        &self,
        command: &str,
        input: Option<std::fs::File>,
        timeout: Duration,
    ) -> Result<RunOutput>;

    /// A channel to the machine's SFTP version 3 file server, which every
    /// implementation serves. It keeps the connection open.
    async fn files(self: Arc<Self>) -> Result<FileChannel>;

    /// Where inside the machine the forwarded local socket is reachable, if
    /// the connection was asked for one. It is bound before the connection
    /// is handed out.
    fn forwarded_socket(&self) -> Option<&str>;
}
