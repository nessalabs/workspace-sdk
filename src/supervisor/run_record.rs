use super::{
    filesystem::{self, atomic, live_lock, read_json},
    protocol::SupervisorContext,
};
use crate::{Error, OutputBytesPage, OutputCursor, OutputPage, Result, RunId, RunState};
use serde::{Deserialize, Serialize};
use std::io::ErrorKind;
use std::{
    fs::OpenOptions,
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
pub const MAX_OUTPUT: u64 = 16 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
pub struct RunClaim {
    pub version: u32,
    pub fingerprint: String,
}
#[derive(Serialize, Deserialize)]
pub struct ProcessIdentity {
    pub supervisor_pid: i32,
    pub pid: i32,
}
pub fn run_directory(context: &SupervisorContext, id: &RunId, create: bool) -> Result<PathBuf> {
    let path = Path::new(&context.control).join("runs").join(id.as_str());
    filesystem::directory(&path, create)?;
    Ok(path)
}
pub fn inspect(path: &Path) -> Result<RunState> {
    let value: RunState = read_json(&path.join("status.json"))?;
    inspect_after_status(path, value)
}
fn inspect_after_status(path: &Path, value: RunState) -> Result<RunState> {
    if value.is_active() && !live_lock(&path.join("active.lock"))? {
        // The supervisor publishes its final status before releasing liveness.
        // Resolve a status read that raced with that publication before declaring
        // an unknown outcome; a stored Lost remains unknown with its own reason.
        let latest: RunState = read_json(&path.join("status.json"))?;
        if !latest.is_active() {
            return Ok(latest);
        }
        Ok(RunState::Lost {
            reason:
                "supervisor disappeared after durable launch claim; execution outcome is unknown"
                    .into(),
        })
    } else {
        Ok(value)
    }
}
fn log(path: &Path) -> Result<(u64, Vec<u8>)> {
    let mut file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok((0, vec![])),
        Err(error) => return Err(error.into()),
    };
    if file.metadata()?.len() > MAX_OUTPUT + 8 {
        return Err(Error::State(
            "run output exceeds its retention bound".into(),
        ));
    }
    let mut header = [0; 8];
    file.read_exact(&mut header)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok((u64::from_be_bytes(header), bytes))
}
pub fn append(path: &Path, chunk: &[u8], capacity: usize) -> Result<()> {
    let (start, mut bytes) = log(path)?;
    bytes.extend_from_slice(chunk);
    let skipped = bytes.len().saturating_sub(capacity);
    let start = start
        .checked_add(skipped as u64)
        .ok_or_else(|| Error::State("run output offset overflow".into()))?;
    let mut snapshot = Vec::with_capacity(8 + bytes.len() - skipped);
    snapshot.extend_from_slice(&start.to_be_bytes());
    snapshot.extend_from_slice(&bytes[skipped..]);
    atomic(path, &snapshot)
}
/// Persist a contiguous retained tail at the runtime's absolute byte positions.
pub(super) fn append_at(path: &Path, from: u64, chunk: &[u8], capacity: usize) -> Result<()> {
    let (mut first, mut bytes) = log(path)?;
    let end = first
        .checked_add(bytes.len() as u64)
        .ok_or_else(|| Error::State("terminal output offset overflow".into()))?;
    if from < end {
        return Err(Error::State("terminal output moved backwards".into()));
    }
    if from != end {
        bytes.clear();
        first = from;
    }
    bytes.extend_from_slice(chunk);
    let skipped = bytes.len().saturating_sub(capacity);
    first = first
        .checked_add(skipped as u64)
        .ok_or_else(|| Error::State("terminal output offset overflow".into()))?;
    let mut record = Vec::with_capacity(8 + bytes.len() - skipped);
    record.extend_from_slice(&first.to_be_bytes());
    record.extend_from_slice(&bytes[skipped..]);
    atomic(path, &record)
}
fn page(path: &Path, cursor: u64, limit: usize) -> Result<(Vec<u8>, u64, bool, u64)> {
    let (first, bytes) = log(path)?;
    let end = first
        .checked_add(bytes.len() as u64)
        .ok_or_else(|| Error::State("run output offset overflow".into()))?;
    if cursor > end {
        return Err(Error::InvalidInput(
            "output cursor is beyond the current stream".into(),
        ));
    }
    let offset = first.max(cursor);
    let begin = (offset - first) as usize;
    let length = limit.min(bytes.len() - begin);
    Ok((
        bytes[begin..begin + length].to_vec(),
        offset + length as u64,
        cursor < first,
        end,
    ))
}
pub fn output(path: &Path, cursor: OutputCursor, limit: u32) -> Result<OutputPage> {
    let page = output_bytes(path, cursor, limit)?;
    Ok(OutputPage {
        stdout: String::from_utf8_lossy(&page.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&page.stderr).into_owned(),
        next: page.next,
        gap: page.gap,
        complete: page.complete,
    })
}
pub fn output_bytes(path: &Path, cursor: OutputCursor, limit: u32) -> Result<OutputBytesPage> {
    if !(2..=filesystem::MAX_CHUNK as u32).contains(&limit) {
        return Err(Error::InvalidInput(
            "output page limit must be 2..=262144 bytes".into(),
        ));
    }
    let state = inspect(path)?;
    let (stdout, out_next, out_gap, out_end) =
        page(&path.join("stdout.log"), cursor.stdout, limit as usize / 2)?;
    let (stderr, err_next, err_gap, err_end) = page(
        &path.join("stderr.log"),
        cursor.stderr,
        limit as usize - limit as usize / 2,
    )?;
    Ok(OutputBytesPage {
        stdout,
        stderr,
        next: OutputCursor {
            stdout: out_next,
            stderr: err_next,
        },
        gap: out_gap || err_gap || path.join("output-gap").exists(),
        complete: state.is_terminal() && out_next == out_end && err_next == err_end,
    })
}

#[cfg(test)]
#[path = "run_record_tests.rs"]
mod tests;
