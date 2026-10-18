use super::client::{failure, remaining};
use crate::{Error, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::Response;
use serde_json::Value;
use std::time::Instant;

const MAX_WIRE: usize = 8 * 1024 * 1024;
const MAX_LINE: usize = 4 * 1024 * 1024;
const MAX_OUTPUT: usize = 2 * 1024 * 1024;
/// Incremental, bounded SSE framing. Transport IDs are not output cursors, and
/// only the native supervisor supplies SDK retained output/run state.
pub(super) async fn collect(
    mut response: Response,
    session: &str,
    process: &str,
    deadline: Instant,
) -> Result<String> {
    let mut parser = Parser::new(session, process);
    let mut wire = 0usize;
    while let Some(chunk) = tokio::time::timeout(remaining(deadline)?, response.chunk())
        .await
        .map_err(|_| {
            Error::Timeout("SPACE event stream deadline; reconcile native execution".into())
        })?
        .map_err(|_| failure("SPACE event stream disconnected; native execution is uncertain"))?
    {
        wire = wire
            .checked_add(chunk.len())
            .ok_or_else(|| failure("SPACE stream byte overflow"))?;
        if wire > MAX_WIRE {
            return Err(failure("SPACE event stream exceeds bound"));
        }
        parser.feed(&chunk)?;
    }
    parser.finish()
}
struct Parser<'a> {
    session: &'a str,
    process: &'a str,
    line: Vec<u8>,
    event: String,
    data: Vec<u8>,
    frames: usize,
    attached: bool,
    started: bool,
    exited: bool,
    closed: bool,
    next_sequence: u64,
    stdout: Vec<u8>,
    stderr_bytes: usize,
}
impl<'a> Parser<'a> {
    fn new(session: &'a str, process: &'a str) -> Self {
        Self {
            session,
            process,
            line: Vec::new(),
            event: String::new(),
            data: Vec::new(),
            frames: 0,
            attached: false,
            started: false,
            exited: false,
            closed: false,
            next_sequence: 1,
            stdout: Vec::new(),
            stderr_bytes: 0,
        }
    }
    fn feed(&mut self, bytes: &[u8]) -> Result<()> {
        for byte in bytes {
            if *byte == b'\n' {
                let mut line = std::mem::take(&mut self.line);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                self.line(&line)?;
            } else {
                if self.line.len() >= MAX_LINE {
                    return Err(failure("SPACE SSE line exceeds bound"));
                }
                self.line.push(*byte);
            }
        }
        Ok(())
    }
    fn line(&mut self, line: &[u8]) -> Result<()> {
        if line.is_empty() {
            return self.dispatch();
        }
        if line.starts_with(b":") {
            return Ok(());
        }
        let line = std::str::from_utf8(line).map_err(|_| failure("SPACE SSE line is not UTF-8"))?;
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => {
                if value.len() > 128 {
                    return Err(failure("SPACE SSE event name exceeds bound"));
                }
                self.event = value.into();
            }
            "data" => {
                if self
                    .data
                    .len()
                    .checked_add(value.len() + 1)
                    .is_none_or(|size| size > MAX_LINE)
                {
                    return Err(failure("SPACE SSE frame exceeds bound"));
                }
                self.data.extend_from_slice(value.as_bytes());
                self.data.push(b'\n');
            }
            // SSE transport ids/retry hints do not provide replay; reconnect is
            // deliberately unsupported. Unknown SSE fields have no semantics.
            "id" | "retry" => {}
            _ => {}
        }
        Ok(())
    }
    fn dispatch(&mut self) -> Result<()> {
        if self.data.is_empty() {
            self.event.clear();
            return Ok(());
        }
        self.frames += 1;
        if self.frames > 20_000 || self.closed {
            return Err(failure("SPACE SSE frame order/bound invalid"));
        }
        self.data.pop();
        let value: Value = serde_json::from_slice(&self.data)
            .map_err(|_| failure("SPACE SSE event JSON is invalid"))?;
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| failure("SPACE SSE event type missing"))?;
        if self.event != kind {
            return Err(failure("SPACE SSE event type mismatch"));
        }
        if self.exited && kind != "closed" {
            return Err(failure("SPACE output/event arrived after process exit"));
        }
        match kind {
            "attached" => {
                if self.attached || value["session_id"] != self.session {
                    return Err(failure("SPACE attached identity/order mismatch"));
                }
                self.attached = true;
            }
            "process_started" => {
                if !self.attached || self.started || value["process_id"] != self.process {
                    return Err(failure("SPACE process start identity/order mismatch"));
                }
                self.started = true;
            }
            "stdout" | "stderr" => {
                if !self.started || value["seq"].as_u64() != Some(self.next_sequence) {
                    return Err(failure("SPACE output sequence gap/order mismatch"));
                }
                self.next_sequence = self
                    .next_sequence
                    .checked_add(1)
                    .ok_or_else(|| failure("SPACE output sequence overflow"))?;
                let encoded = value["data_base64"]
                    .as_str()
                    .ok_or_else(|| failure("SPACE output is not base64"))?;
                let bytes = STANDARD
                    .decode(encoded)
                    .map_err(|_| failure("SPACE output is not base64"))?;
                if kind == "stdout" {
                    if self
                        .stdout
                        .len()
                        .checked_add(bytes.len())
                        .is_none_or(|size| size > MAX_OUTPUT)
                    {
                        return Err(failure("SPACE RPC output exceeds bound"));
                    }
                    self.stdout.extend_from_slice(&bytes);
                } else {
                    self.stderr_bytes = self
                        .stderr_bytes
                        .checked_add(bytes.len())
                        .ok_or_else(|| failure("SPACE RPC error output overflow"))?;
                    if self.stderr_bytes > MAX_OUTPUT {
                        return Err(failure("SPACE RPC error output exceeds bound"));
                    }
                }
            }
            "closed" => {
                if !self.exited || value["reason"] != "backend_closed" {
                    return Err(failure(
                        "SPACE closed before successful completion or with an unsupported reason",
                    ));
                }
                self.closed = true;
            }
            "process_exited" => {
                if !self.started
                    || value["process_id"] != self.process
                    || value["status"] != "exited"
                    || value["exit_code"].as_i64() != Some(0)
                {
                    return Err(failure(
                        "SPACE RPC did not confirm successful process completion",
                    ));
                }
                self.exited = true;
            }
            _ => {
                return Err(failure(
                    "SPACE SSE reported unsupported/error/resynchronization state",
                ))
            }
        }
        self.event.clear();
        self.data.clear();
        Ok(())
    }
    // A matching successful process exit plus complete EOF is sufficient. The
    // pinned server also sends backend_closed; when present it is validated
    // above and never substitutes for process completion.
    fn finish(self) -> Result<String> {
        if !self.line.is_empty() || !self.data.is_empty() || !self.exited {
            return Err(failure(
                "SPACE RPC stream ended without a complete successful terminal frame",
            ));
        }
        String::from_utf8(self.stdout).map_err(|_| failure("SPACE native response is not UTF-8"))
    }
}
