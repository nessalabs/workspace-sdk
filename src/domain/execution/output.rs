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

/// A bounded page of retained run output and its continuation cursor.
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
