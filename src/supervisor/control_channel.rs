//! Bounded framing shared by dedicated native supervisor control processes.
use super::filesystem;
use crate::{Error, Result};
use std::{
    io::{ErrorKind, Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
pub(super) const FRAME_LIMIT: usize = 128 * 1024;
pub(super) const FRAME_BUDGET: Duration = Duration::from_millis(250);
pub(super) const RESPONSE_BUDGET: Duration = Duration::from_secs(2);

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

pub(super) fn read_frame(stream: &mut UnixStream, budget: Duration) -> Result<Vec<u8>> {
    let deadline = Instant::now() + budget;
    let mut frame = Vec::new();
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| Error::Timeout("native control frame timed out".into()))?;
        stream.set_read_timeout(Some(remaining))?;
        let mut bytes = [0; 4096];
        let size = match stream.read(&mut bytes) {
            Ok(size) => size,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                return Err(Error::Timeout("native control frame timed out".into()));
            }
            Err(error) => return Err(error.into()),
        };
        if size == 0 {
            return Err(Error::State("incomplete native control frame".into()));
        }
        if frame.len() + size > FRAME_LIMIT {
            return Err(Error::InvalidInput(
                "native control frame exceeds its bound".into(),
            ));
        }
        frame.extend_from_slice(&bytes[..size]);
        if frame.last() == Some(&b'\n') {
            return Ok(frame);
        }
    }
}
pub(super) fn write_frame(stream: &mut UnixStream, bytes: &[u8]) -> Result<()> {
    let deadline = Instant::now() + FRAME_BUDGET;
    let mut offset = 0;
    while offset < bytes.len() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| Error::Timeout("native control acknowledgement timed out".into()))?;
        stream.set_write_timeout(Some(remaining))?;
        match stream.write(&bytes[offset..]) {
            Ok(0) => return Err(Error::State("native control channel closed".into())),
            Ok(size) => offset += size,
            Err(error) if error.kind() == ErrorKind::Interrupted => (),
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                return Err(Error::Timeout(
                    "native control acknowledgement timed out".into(),
                ));
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
