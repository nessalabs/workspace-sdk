//! Bounded transient control channel to the separately owned PTY supervisor.
use super::{
    filesystem,
    protocol::{SupervisorFault, SupervisorOperation, SupervisorResponse, SupervisorValue},
};
use crate::{Error, InputOutcome, Result, RunId};
use std::{
    fs,
    io::{ErrorKind, Read, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const FRAME_LIMIT: usize = 128 * 1024;
const FRAME_BUDGET: Duration = Duration::from_millis(250);
// The reply includes queue service and a separately bounded native operation.
const RESPONSE_BUDGET: Duration = Duration::from_secs(2);

/// Only used in the dedicated CLI process, never in a threaded SDK host.
pub(super) struct DirectoryGuard(PathBuf);
impl DirectoryGuard {
    pub fn enter(path: &Path) -> Result<Self> {
        filesystem::directory(path, false)?;
        let previous = std::env::current_dir()?;
        std::env::set_current_dir(path)?;
        Ok(Self(previous))
    }
}
impl Drop for DirectoryGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

pub(super) fn listen() -> Result<UnixListener> {
    let socket = UnixListener::bind("pty.sock")?;
    fs::set_permissions("pty.sock", fs::Permissions::from_mode(0o600))?;
    socket.set_nonblocking(true)?;
    Ok(socket)
}

fn read_frame(stream: &mut UnixStream, budget: Duration) -> Result<Vec<u8>> {
    let deadline = Instant::now() + budget;
    let mut frame = Vec::new();
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| Error::Timeout("terminal control frame timed out".into()))?;
        stream.set_read_timeout(Some(remaining))?;
        let mut bytes = [0; 4096];
        let size = match stream.read(&mut bytes) {
            Ok(size) => size,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                return Err(Error::Timeout("terminal control frame timed out".into()));
            }
            Err(error) => return Err(error.into()),
        };
        if size == 0 {
            return Err(Error::State("incomplete terminal control frame".into()));
        }
        if frame.len() + size > FRAME_LIMIT {
            return Err(Error::InvalidInput(
                "terminal control frame exceeds its bound".into(),
            ));
        }
        frame.extend_from_slice(&bytes[..size]);
        if frame.last() == Some(&b'\n') {
            return Ok(frame);
        }
    }
}
fn write_frame(stream: &mut UnixStream, bytes: &[u8]) -> Result<()> {
    let deadline = Instant::now() + FRAME_BUDGET;
    let mut offset = 0;
    while offset < bytes.len() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| Error::Timeout("terminal control acknowledgement timed out".into()))?;
        stream.set_write_timeout(Some(remaining))?;
        match stream.write(&bytes[offset..]) {
            Ok(0) => return Err(Error::State("terminal control channel closed".into())),
            Ok(size) => offset += size,
            Err(error) if error.kind() == ErrorKind::Interrupted => (),
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                return Err(Error::Timeout(
                    "terminal control acknowledgement timed out".into(),
                ));
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub(super) fn receive(stream: &mut UnixStream) -> Result<SupervisorOperation> {
    stream.set_nonblocking(false)?;
    let mut bytes = read_frame(stream, FRAME_BUDGET)?;
    let result = serde_json::from_slice(&bytes)
        .map_err(|_| Error::InvalidInput("invalid terminal control request".into()));
    bytes.fill(0);
    result
}
pub(super) fn respond(stream: &mut UnixStream, result: Result<SupervisorValue>) {
    if let Ok(mut bytes) = serde_json::to_vec(&SupervisorResponse::from_result(result)) {
        bytes.push(b'\n');
        let _ = write_frame(stream, &bytes);
    }
}

pub(super) fn request(path: &Path, operation: SupervisorOperation) -> Result<SupervisorValue> {
    let input = matches!(operation, SupervisorOperation::TerminalInput { .. });
    let connection = (|| {
        let directory = DirectoryGuard::enter(path)?;
        let socket = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)?;
        // Bound connection backlog waits before any input is serialized or sent.
        socket
            .connect_timeout(&socket2::SockAddr::unix("pty.sock")?, FRAME_BUDGET)
            .map_err(|_| {
                Error::Unsupported("this run has no available terminal control owner".into())
            })?;
        let descriptor: std::os::fd::OwnedFd = socket.into();
        Ok((directory, UnixStream::from(descriptor)))
    })();
    let (_directory, mut stream) = match connection {
        Ok(connection) => connection,
        Err(error) if input => {
            return Ok(SupervisorValue::InputRejected(SupervisorFault::from_error(
                error,
            )))
        }
        Err(error) => return Err(error),
    };
    let exchange = (|| {
        let mut bytes = serde_json::to_vec(&operation)?;
        bytes.push(b'\n');
        let sent = write_frame(&mut stream, &bytes);
        bytes.fill(0);
        sent?;
        let bytes = read_frame(&mut stream, RESPONSE_BUDGET)?;
        let response: SupervisorResponse = serde_json::from_slice(&bytes)
            .map_err(|_| Error::State("invalid terminal control acknowledgement".into()))?;
        let value = response.into_result()?;
        // A socket reply arrives after dispatch. Only our local pre-dispatch
        // checks may establish refusal; a wrong success value is ambiguous.
        if input && !matches!(&value, SupervisorValue::Input(_)) {
            return Err(Error::State(
                "invalid terminal input acknowledgement".into(),
            ));
        }
        Ok(value)
    })();
    if input && exchange.is_err() {
        return Ok(SupervisorValue::Input(InputOutcome::Unknown));
    }
    exchange
}

pub(super) fn same_run(expected: &RunId, received: &RunId) -> Result<()> {
    if expected != received {
        return Err(Error::InvalidInput(
            "terminal control belongs to another run".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "terminal_control_tests.rs"]
mod tests;
