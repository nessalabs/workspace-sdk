//! A long-lived PTY owner independent of individual provider request processes.
use super::{
    filesystem::{atomic, write_json},
    protocol::{SupervisorExecution, SupervisorOperation, SupervisorValue},
    run_record, terminal_command, terminal_control,
};
use crate::{Error, InputOutcome, Result, RunId, RunState, TerminalSize};
use pty_runtime::{
    AttachPosition, DrainOutcome, ExitStatus, OutputEvent, ReplayPage, Runtime, RuntimeOptions,
    Session, SessionOptions,
};
use std::{
    fs,
    io::ErrorKind,
    path::Path,
    time::{Duration, Instant},
};

pub(super) fn work(
    path: &Path,
    cwd: &Path,
    id: &RunId,
    execution: &SupervisorExecution,
    limit: usize,
    size: TerminalSize,
) -> Result<()> {
    let command = match terminal_command::command(cwd, execution) {
        Ok(command) => command,
        Err(Error::NotFound(_)) if cwd.is_dir() => {
            // Resolution proved no child was launched; retain a deterministic
            // failure receipt rather than claiming uncertain execution.
            run_record::append_at(
                &path.join("stdout.log"),
                0,
                b"workspace-supervisor: executable was not found\n",
                limit,
            )?;
            write_json(
                &path.join("status.json"),
                &RunState::Exited { exit_code: 127 },
            )?;
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let _directory = terminal_control::DirectoryGuard::enter(path)?;
    let listener = terminal_control::listen()?;
    // Guardian staging must precede Tokio and any session reader threads.
    let runtime = Runtime::new(
        vec![cwd.to_owned()],
        RuntimeOptions {
            max_sessions: 1,
            max_observers: 2,
            replay_bytes: limit,
            output_page_bytes: 65536,
            input_bytes: 65536,
            input_slots: 2,
            ..Default::default()
        },
    )
    .map_err(native_error)?;
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()?;
    let mut options = SessionOptions::raw(
        pty_runtime::TerminalSize::new(size.columns(), size.rows())
            .map_err(|_| Error::InvalidInput("invalid terminal size".into()))?,
    );
    options.replay_bytes = limit;
    options.max_observers = 2;
    options.process.input_chunk = 65536;
    options.process.input_bytes = 65536;
    options.process.input_slots = 2;
    options.process.write_timeout = Duration::from_millis(500);
    let session = runtime
        .spawn(
            pty_runtime::SessionId::new(id.as_str().to_owned())
                .map_err(|_| Error::InvalidInput("invalid terminal run ID".into()))?,
            &command,
            options,
        )
        .map_err(native_error)?;
    let mut output = session
        .attach(AttachPosition::Oldest)
        .map_err(native_error)?;
    write_json(&path.join("status.json"), &RunState::Running)?;
    write_json(
        &path.join("process.json"),
        &run_record::ProcessIdentity {
            supervisor_pid: std::process::id() as i32,
            pid: session.process_id().map_err(native_error)? as i32,
        },
    )?;
    let started = Instant::now();
    let mut terminal = None;
    let final_state = loop {
        if terminal.is_none()
            && path.join("cancel").exists()
            && session.status().map_err(native_error)?.exit.is_none()
        {
            session.cancel().map_err(native_error)?;
            terminal = Some(RunState::Canceled);
        }
        if terminal.is_none()
            && session.status().map_err(native_error)?.exit.is_none()
            && execution
                .command
                .timeout_seconds
                .is_some_and(|seconds| started.elapsed() >= Duration::from_secs(seconds))
        {
            session.cancel().map_err(native_error)?;
            terminal = Some(RunState::TimedOut);
        }
        // Finite pumping gives cancellation and controls time even under output saturation.
        let mut finished = None;
        for _ in 0..16 {
            match output.try_next().map_err(native_error)? {
                Some(OutputEvent::Replay(ReplayPage::Bytes { from, bytes, .. })) => {
                    run_record::append_at(&path.join("stdout.log"), from.offset, &bytes, limit)?
                }
                Some(OutputEvent::Replay(ReplayPage::Gap { to, .. })) => {
                    run_record::append_at(&path.join("stdout.log"), to.offset, &[], limit)?
                }
                Some(OutputEvent::Complete(completion)) => {
                    finished = Some(completion.status);
                    break;
                }
                None => break,
                _ => (),
            }
        }
        if let Some(status) = finished {
            if status.drain != Some(DrainOutcome::Eof) {
                atomic(&path.join("output-gap"), b"incomplete drain")?;
            }
            let state = match (
                status.exit,
                status.supervision_error,
                status.admission_error,
            ) {
                (Some(exit), None, None) => terminal.unwrap_or(RunState::Exited {
                    exit_code: match exit {
                        ExitStatus::Code(code) => code,
                        ExitStatus::Signal(signal) => -signal,
                    },
                }),
                _ => RunState::Lost {
                    reason: "terminal supervision did not establish process exit".into(),
                },
            };
            break state;
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let response = terminal_control::receive(&mut stream)
                    .and_then(|request| control(&executor, &session, id, request));
                terminal_control::respond(&mut stream, response);
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(error) => return Err(error.into()),
        }
    };
    runtime.shutdown();
    drop(listener);
    // Shutdown already established process cleanup. Unlink cannot invalidate
    // that fact, even if the socket was removed or replaced by a directory.
    let _ = fs::remove_file("pty.sock");
    write_json(&path.join("status.json"), &final_state)?;
    Ok(())
}
fn control(
    executor: &tokio::runtime::Runtime,
    session: &Session,
    id: &RunId,
    operation: SupervisorOperation,
) -> Result<SupervisorValue> {
    match operation {
        SupervisorOperation::TerminalInput {
            id: received,
            input: Some(input),
        } => {
            terminal_control::same_run(id, &received)?;
            if input.expose().len() > 65536 {
                return Err(Error::InvalidInput("terminal input exceeds 64 KiB".into()));
            }
            if session
                .status()
                .map_err(native_error)?
                .cancellation_requested
            {
                return Ok(SupervisorValue::Input(InputOutcome::Accepted { bytes: 0 }));
            }
            let operation = session.write(input.expose()).map_err(native_error)?;
            let outcome = executor.block_on(async {
                tokio::time::timeout(Duration::from_millis(500), operation).await
            });
            Ok(SupervisorValue::Input(match outcome {
                Ok(value) => InputOutcome::Accepted {
                    bytes: value.written,
                },
                Err(_) => InputOutcome::Unknown,
            }))
        }
        SupervisorOperation::TerminalResize { id: received, size } => {
            terminal_control::same_run(id, &received)?;
            let size = pty_runtime::TerminalSize::new(size.columns(), size.rows())
                .map_err(|_| Error::InvalidInput("invalid terminal size".into()))?;
            let operation = session.resize(size).map_err(native_error)?;
            executor
                .block_on(async {
                    tokio::time::timeout(Duration::from_millis(500), operation).await
                })
                .map_err(|_| Error::Timeout("terminal resize acknowledgement unavailable".into()))?
                .map_err(|_| Error::State("terminal resize failed".into()))?;
            Ok(SupervisorValue::Empty)
        }
        _ => Err(Error::InvalidInput(
            "invalid terminal control operation".into(),
        )),
    }
}
fn native_error(error: pty_runtime::RuntimeError) -> Error {
    Error::State(format!("terminal runtime: {error}"))
}
