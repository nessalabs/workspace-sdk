use super::{ICommandRunner, NativeCommand};
use crate::{Error, ExecResult, Result};
use async_trait::async_trait;
use std::{
    io::{self, Error as IoError, ErrorKind},
    process::Stdio,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
};

/// Default [`ICommandRunner`] backed by Tokio's operating-system process API.
///
/// It sends standard input through a pipe, waits for EOF and process completion,
/// and bounds retained output to 2 MiB per stream. Exceeding that limit returns
/// an error instead of a truncated protocol response. A transport timeout stops
/// the native child; a detached target workload may continue running.
#[derive(Default)]
pub struct SystemCommandRunner;
async fn drain(mut stream: impl AsyncRead + Unpin) -> io::Result<(Vec<u8>, bool)> {
    let mut data = Vec::new();
    let mut chunk = [0; 8192];
    let mut truncated = false;
    loop {
        let size = stream.read(&mut chunk).await?;
        if size == 0 {
            break;
        }
        let keep = size.min((2 * 1024 * 1024_usize).saturating_sub(data.len()));
        data.extend_from_slice(&chunk[..keep]);
        truncated |= keep < size;
    }
    Ok((data, truncated))
}
#[async_trait]
impl ICommandRunner for SystemCommandRunner {
    async fn run(&self, request: NativeCommand) -> Result<ExecResult> {
        let mut command = Command::new(&request.program);
        command
            .args(&request.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                Error::NotFound(format!(
                    "required executable {} is unavailable",
                    request.program
                ))
            } else {
                Error::Io(e)
            }
        })?;
        let mut input = child.stdin.take().expect("piped stdin");
        let output = child.stdout.take().expect("piped stdout");
        let errors = child.stderr.take().expect("piped stderr");
        let future = async {
            let write = async {
                input.write_all(&request.stdin).await?;
                input.shutdown().await?;
                drop(input);
                Ok::<_, IoError>(())
            };
            let ((), out, err, status) =
                tokio::try_join!(write, drain(output), drain(errors), child.wait())?;
            Ok::<_, IoError>((out, err, status))
        };
        match tokio::time::timeout(request.timeout, future).await {
            Ok(Ok(((stdout, out_cut), (stderr, err_cut), status))) => {
                if out_cut || err_cut {
                    return Err(Error::State(
                        "native protocol output exceeded the 2 MiB limit".into(),
                    ));
                }
                Ok(ExecResult {
                    exit_code: status.code(),
                    stdout: String::from_utf8_lossy(&stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&stderr).into_owned(),
                    timed_out: false,
                    output_truncated: false,
                })
            }
            Ok(Err(e)) => Err(Error::Io(e)),
            Err(_) => {
                let _ = child.kill().await;
                Err(Error::Timeout("native transport; a managed launch may have succeeded: reconnect using its run ID".into()))
            }
        }
    }
}
