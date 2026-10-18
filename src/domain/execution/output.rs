//! Value objects for reading a run’s retained output incrementally.
use serde::{Deserialize, Serialize};
use std::fmt;

/// Independent absolute byte positions in a run’s stdout and stderr streams.
///
/// Start with `Default` to read from zero and persist [`OutputPage::next`] after
/// each page. Offsets do not reset when old output is discarded.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OutputCursor {
    /// Next unread absolute byte offset in standard output.
    pub stdout: u64,
    /// Next unread absolute byte offset in standard error.
    pub stderr: u64,
}

/// A bounded, lossy UTF-8 text page of retained run output.
///
/// Split code points or invalid bytes may become replacement characters. Use
/// [`OutputBytesPage`] when exact bytes are required.
///
/// If `gap` is true, some requested bytes were already discarded; clients should
/// show an output-gap marker instead of presenting the log as complete.
#[derive(Clone, Serialize, Deserialize)]
pub struct OutputPage {
    /// Available standard-output text for this page.
    pub stdout: String,
    /// Available standard-error text for this page.
    pub stderr: String,
    /// Cursor to supply for the next read.
    pub next: OutputCursor,
    /// Whether retention dropped bytes between the requested and available positions.
    pub gap: bool,
    /// Whether the process has a confirmed terminal state and output is fully consumed.
    pub complete: bool,
}

impl fmt::Debug for OutputPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OutputPage")
            .field("stdout_bytes", &self.stdout.len())
            .field("stderr_bytes", &self.stderr.len())
            .field("next", &self.next)
            .field("gap", &self.gap)
            .field("complete", &self.complete)
            .finish()
    }
}

/// A byte-preserving page of independently retained stdout and stderr.
///
/// Cursors count absolute raw bytes. `gap` reports discarded requested output;
/// `complete` requires a confirmed terminal process and both streams drained.
/// The page budget is combined across both streams. Output may contain secrets;
/// Debug reports lengths and cursor metadata only.
#[derive(Clone)]
pub struct OutputBytesPage {
    /// Exact retained stdout bytes for this page.
    pub stdout: Vec<u8>,
    /// Exact retained stderr bytes, never merged into stdout.
    pub stderr: Vec<u8>,
    /// Absolute cursors for the next read; reading does not mutate a cursor.
    pub next: OutputCursor,
    /// Requested bytes were dropped by retention.
    pub gap: bool,
    /// Process is confirmed terminal and both streams have been consumed.
    pub complete: bool,
}
impl fmt::Debug for OutputBytesPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OutputBytesPage")
            .field("stdout_bytes", &self.stdout.len())
            .field("stderr_bytes", &self.stderr.len())
            .field("next", &self.next)
            .field("gap", &self.gap)
            .field("complete", &self.complete)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn byte_page_debug_omits_output_contents() {
        let page = OutputBytesPage {
            stdout: b"stdout-sensitive-marker".to_vec(),
            stderr: b"stderr-sensitive-marker".to_vec(),
            next: OutputCursor::default(),
            gap: false,
            complete: false,
        };
        let debug = format!("{page:?}");
        assert!(!debug.contains("sensitive-marker"));
        assert!(debug.contains("stdout_bytes"));
        assert!(debug.contains("stderr_bytes"));
    }
}
