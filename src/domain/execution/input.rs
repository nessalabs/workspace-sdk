//! Acknowledgement for closing one explicitly piped managed run's input.
use serde::{Deserialize, Serialize};

/// Observation of an idempotent input EOF request, not process termination.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputCloseOutcome {
    /// The exact pipe is closed. This does not prove the child consumed input,
    /// completed successfully, or released external resources.
    Closed,
    /// Closing may have happened, but the acknowledgement cannot be established.
    /// Retrying close on the same RunRef is safe; never replay earlier input.
    Unknown,
}
