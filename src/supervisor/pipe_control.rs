//! Transient bounded input controls for one detached native pipe owner.
use super::{
    control_channel::{self, DirectoryGuard, FRAME_BUDGET, RESPONSE_BUDGET},
    protocol::{SupervisorFault, SupervisorOperation, SupervisorResponse, SupervisorValue},
    run_record,
};
use crate::{Error, InputCloseOutcome, InputOutcome, Result, RunId, RunState};
use std::{
    fs,
    io::{ErrorKind, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::Path,
    process::ChildStdin,
};
const SOCKET: &str = "stdin.sock";
const INPUT_LIMIT: usize = 65536;

pub(super) struct PipeListener {
    socket: UnixListener,
}
impl Drop for PipeListener {
    fn drop(&mut self) {
        let _ = fs::remove_file(SOCKET);
    }
}
pub(super) fn listen() -> Result<PipeListener> {
    let listener = PipeListener {
        socket: UnixListener::bind(SOCKET)?,
    };
    fs::set_permissions(SOCKET, fs::Permissions::from_mode(0o600))?;
    listener.socket.set_nonblocking(true)?;
    Ok(listener)
}
fn rejected(close: bool, error: Error) -> SupervisorValue {
    let fault = SupervisorFault::from_error(error);
    if close {
        SupervisorValue::InputCloseRejected(fault)
    } else {
        SupervisorValue::InputRejected(fault)
    }
}
fn unknown(close: bool) -> SupervisorValue {
    if close {
        SupervisorValue::InputClosed(InputCloseOutcome::Unknown)
    } else {
        SupervisorValue::Input(InputOutcome::Unknown)
    }
}
fn receive(stream: &mut UnixStream) -> Result<SupervisorOperation> {
    stream.set_nonblocking(false)?;
    let mut bytes = control_channel::read_frame(stream, FRAME_BUDGET)?;
    let operation = serde_json::from_slice(&bytes)
        .map_err(|_| Error::InvalidInput("invalid pipe control request".into()));
    bytes.fill(0);
    operation
}
fn respond(stream: &mut UnixStream, result: Result<SupervisorValue>) {
    if let Ok(mut bytes) = serde_json::to_vec(&SupervisorResponse::from_result(result)) {
        bytes.push(b'\n');
        let _ = control_channel::write_frame(stream, &bytes);
    }
}
/// Service at most one request so output, cancellation and deadlines keep time.
pub(super) fn serve_one(
    listener: &PipeListener,
    expected: &RunId,
    stdin: &mut Option<ChildStdin>,
) -> Result<()> {
    let mut stream = match listener.socket.accept() {
        Ok((stream, _)) => stream,
        Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {
            return Ok(())
        }
        Err(error) => return Err(error.into()),
    };
    let result = receive(&mut stream).map(|operation| dispatch(expected, stdin, operation));
    respond(&mut stream, result);
    Ok(())
}
fn dispatch(
    expected: &RunId,
    stdin: &mut Option<ChildStdin>,
    operation: SupervisorOperation,
) -> SupervisorValue {
    match operation {
        SupervisorOperation::PipeCloseInput { id } => {
            if &id != expected {
                return rejected(
                    true,
                    Error::InvalidInput("pipe control belongs to another run".into()),
                );
            }
            // This owner serializes requests. All previously acknowledged bytes
            // already entered the kernel pipe before dropping its writer here.
            *stdin = None;
            SupervisorValue::InputClosed(InputCloseOutcome::Closed)
        }
        SupervisorOperation::PipeInput { id, input } => {
            if &id != expected {
                return rejected(
                    false,
                    Error::InvalidInput("pipe control belongs to another run".into()),
                );
            }
            let Some(input) = input else {
                return rejected(false, Error::InvalidInput("pipe input is missing".into()));
            };
            let bytes = input.expose();
            if bytes.is_empty() || bytes.len() > INPUT_LIMIT {
                return rejected(
                    false,
                    Error::InvalidInput("pipe input must contain 1..=65536 bytes".into()),
                );
            }
            let Some(writer) = stdin.as_mut() else {
                return rejected(
                    false,
                    Error::Conflict("managed pipe input is closed".into()),
                );
            };
            // One nonblocking write: no hidden queue and no accepted prefix is
            // submitted a second time. A lost reply is classified by the caller.
            match writer.write(bytes) {
                Ok(0) => rejected(false, Error::State("managed pipe accepted no input".into())),
                Ok(bytes) => SupervisorValue::Input(InputOutcome::Accepted { bytes }),
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    SupervisorValue::Input(InputOutcome::Accepted { bytes: 0 })
                }
                Err(error) if error.kind() == ErrorKind::BrokenPipe => {
                    *stdin = None;
                    rejected(
                        false,
                        Error::Conflict("managed pipe input is closed".into()),
                    )
                }
                Err(error) => rejected(false, error.into()),
            }
        }
        _ => rejected(
            false,
            Error::InvalidInput("operation is not a pipe control".into()),
        ),
    }
}

pub(super) fn request(path: &Path, operation: SupervisorOperation) -> Result<SupervisorValue> {
    let close = matches!(&operation, SupervisorOperation::PipeCloseInput { .. });
    let preflight = (|| {
        let id = match &operation {
            SupervisorOperation::PipeInput { id, input } => {
                let input = input
                    .as_ref()
                    .ok_or_else(|| Error::InvalidInput("pipe input is missing".into()))?;
                if input.expose().is_empty() || input.expose().len() > INPUT_LIMIT {
                    return Err(Error::InvalidInput(
                        "pipe input must contain 1..=65536 bytes".into(),
                    ));
                }
                id
            }
            SupervisorOperation::PipeCloseInput { id } => id,
            _ => {
                return Err(Error::InvalidInput(
                    "operation is not a pipe control".into(),
                ))
            }
        };
        if path.file_name().and_then(|name| name.to_str()) != Some(id.as_str()) {
            return Err(Error::InvalidInput(
                "pipe control belongs to another run".into(),
            ));
        }
        run_record::require_piped(path)?;
        run_record::inspect(path)
    })();
    match preflight {
        Err(error) => return Ok(rejected(close, error)),
        Ok(state) if state.is_terminal() => {
            return Ok(if close {
                SupervisorValue::InputClosed(InputCloseOutcome::Closed)
            } else {
                rejected(
                    false,
                    Error::Conflict("managed pipe input is closed".into()),
                )
            })
        }
        Ok(RunState::Lost { .. }) => return Ok(unknown(close)),
        Ok(_) => (),
    }
    let connection = (|| {
        let directory = DirectoryGuard::enter(path)?;
        let socket = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)?;
        socket
            .connect_timeout(&socket2::SockAddr::unix(SOCKET)?, FRAME_BUDGET)
            .map_err(|_| {
                Error::Unsupported("this run has no available pipe control owner".into())
            })?;
        let descriptor: std::os::fd::OwnedFd = socket.into();
        Ok((directory, UnixStream::from(descriptor)))
    })();
    let (_directory, mut stream) = match connection {
        Ok(connection) => connection,
        Err(error) => return Ok(rejected(close, error)),
    };
    let exchange = (|| {
        let mut bytes = serde_json::to_vec(&operation)?;
        bytes.push(b'\n');
        let sent = control_channel::write_frame(&mut stream, &bytes);
        bytes.fill(0);
        sent?;
        let bytes = control_channel::read_frame(&mut stream, RESPONSE_BUDGET)?;
        let response: SupervisorResponse = serde_json::from_slice(&bytes)
            .map_err(|_| Error::State("invalid pipe control acknowledgement".into()))?;
        let value = response.into_result()?;
        let expected = if close {
            matches!(
                &value,
                SupervisorValue::InputClosed(_) | SupervisorValue::InputCloseRejected(_)
            )
        } else {
            matches!(
                &value,
                SupervisorValue::Input(_) | SupervisorValue::InputRejected(_)
            )
        };
        if !expected {
            return Err(Error::State(
                "unexpected pipe control acknowledgement".into(),
            ));
        }
        Ok(value)
    })();
    Ok(exchange.unwrap_or_else(|_| unknown(close)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supervisor::filesystem;

    fn fixture(piped: bool, state: RunState) -> (tempfile::TempDir, std::path::PathBuf, RunId) {
        let temporary = tempfile::tempdir().unwrap();
        let id = RunId::new("owned-pipe-test").unwrap();
        let path = temporary.path().join(id.as_str());
        fs::create_dir(&path).unwrap();
        filesystem::write_json(
            &path.join("claim.json"),
            &run_record::RunClaim {
                version: 1,
                fingerprint: "fixture-mode-only".into(),
                piped,
            },
        )
        .unwrap();
        filesystem::write_json(&path.join("status.json"), &state).unwrap();
        (temporary, path, id)
    }

    #[test]
    fn disappeared_pipe_owner_never_acknowledges_eof() {
        let (_temporary, path, id) = fixture(true, RunState::Starting);
        // Actual native observation: no live owner holds active.lock. No child
        // was launched; this fixture qualifies observation handling only.
        assert!(matches!(
            request(&path, SupervisorOperation::PipeCloseInput { id }).unwrap(),
            SupervisorValue::InputClosed(InputCloseOutcome::Unknown)
        ));
        assert!(!path.join(SOCKET).exists());
    }

    #[test]
    fn terminal_nonpipe_claim_cannot_acknowledge_pipe_eof() {
        // Ordinary and PTY claims both omit the piped mode flag. A terminal
        // process alone cannot grant a pipe capability it never owned.
        for state in [RunState::Exited { exit_code: 0 }, RunState::Canceled] {
            let (_temporary, path, id) = fixture(false, state);
            assert!(matches!(
                request(&path, SupervisorOperation::PipeCloseInput { id }).unwrap(),
                SupervisorValue::InputCloseRejected(_)
            ));
            assert!(!path.join(SOCKET).exists());
            assert!(
                !path.join("active.lock").exists(),
                "mode refusal precedes liveness I/O"
            );
        }
    }
}
