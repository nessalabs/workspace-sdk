//! A live connection into a workspace's machine: long-running streams, short
//! commands, a file channel, and one local socket reachable from inside.
//!
//! The managed runs of [`IBackend`](super::IBackend) are the SDK's own
//! supervised processes. A connection is for what an application runs
//! itself, such as an agent that talks over standard input and output for
//! hours, and for which the application keeps its own protocol.
use crate::Result;
use async_trait::async_trait;
use std::{any::Any, path::PathBuf, sync::Arc};
use tokio::io::{AsyncRead, AsyncWrite};

/// Bytes into a connection, such as a process's standard input.
pub type ConnectionInput = Box<dyn AsyncWrite + Send + Unpin>;
/// Bytes out of a connection, such as a process's standard output.
pub type ConnectionOutput = Box<dyn AsyncRead + Send + Unpin>;

/// One local socket a connection makes reachable inside the machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocketForward {
    /// The listening Unix socket on this machine.
    pub local: PathBuf,
}

/// A process started by [`IConnection::stream`]. Dropping it ends the
/// process and closes its input, which tells it to stop.
pub struct ConnectionStream {
    /// Its standard input, after the first line.
    pub input: ConnectionInput,
    /// Its standard output.
    pub output: ConnectionOutput,
    /// What keeps the process, and the connection it uses, alive.
    pub keep: Box<dyn Any + Send>,
}

/// The two ends of a file channel; see [`IConnection::files`].
pub struct FileChannel {
    /// What the machine's file server sends.
    pub from_machine: ConnectionOutput,
    /// What is sent to the machine's file server.
    pub to_machine: ConnectionInput,
    /// What keeps the channel open; dropping it closes the channel.
    pub keep: Box<dyn Any + Send>,
}

/// The most of a [`IConnection::run`] command's output that is kept: its
/// last bytes, so a failure's end is never lost.
pub const MAX_RUN_OUTPUT: usize = 64 * 1024;

/// A connection into one workspace's machine.
///
/// Every `command` is a POSIX `sh` script, run in the machine account's home
/// directory whatever that account's login shell is; the caller changes
/// directory itself. Nothing an application passes in a command line is
/// secret: a secret goes in a stream's first line.
#[async_trait]
pub trait IConnection: Send + Sync {
    /// Start `command`, writing `first_line` and a newline to its standard
    /// input before anything else, so a secret the command reads first never
    /// appears in a command line or on a disk. The process runs until it
    /// ends or the stream is dropped; the stream keeps the connection open.
    /// `first_line` holds no newline.
    async fn stream(
        self: Arc<Self>,
        command: &str,
        first_line: Option<&[u8]>,
    ) -> Result<ConnectionStream>;

    /// Run `command` to its end, `input` on its standard input, and answer
    /// the last [`MAX_RUN_OUTPUT`] bytes of its standard output, decoded
    /// lossily. Its exit status is not part of the answer: a command that
    /// must be checked prints its own result.
    async fn run(&self, command: &str, input: Option<std::fs::File>) -> Result<String>;

    /// An SFTP channel to the machine's file server; it keeps the
    /// connection open.
    async fn files(self: Arc<Self>) -> Result<FileChannel>;

    /// Where inside the machine the forwarded local socket is reachable, if
    /// the connection was asked for one.
    fn forwarded_socket(&self) -> Option<&str>;
}
