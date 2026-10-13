//! Values controlling native authentication interaction without owning a PTY.

use crate::{Error, Result};

/// Validated terminal dimensions. Adapters may impose smaller native limits.
/// This value carries no Ghostty or operating-system representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeTerminalSize {
    columns: u16,
    rows: u16,
}

impl RuntimeTerminalSize {
    /// Construct a positive size, bounded to 1,000 columns/rows and 250,000 cells.
    pub fn new(columns: u16, rows: u16) -> Result<Self> {
        if columns == 0
            || rows == 0
            || columns > 1_000
            || rows > 1_000
            || u32::from(columns) * u32::from(rows) > 250_000
        {
            return Err(Error::InvalidInput(
                "native terminal size exceeds its bounds".into(),
            ));
        }
        Ok(Self { columns, rows })
    }

    /// Number of character columns.
    pub fn columns(self) -> u16 {
        self.columns
    }

    /// Number of character rows.
    pub fn rows(self) -> u16 {
        self.rows
    }
}

/// Delivery observation for one transient input submission. The service never
/// retries input automatically. Terminal echo can copy submitted bytes to output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInputOutcome {
    /// The host accepted exactly this prefix into its ordered input channel.
    /// This does not prove the child consumed it. A short count permits submitting
    /// only the remaining suffix, never the already accepted prefix.
    Accepted {
        /// Prefix length in bytes, bounded by the submitted input length.
        bytes: usize,
    },
    /// Delivery may have happened but its acknowledgement is unavailable.
    /// Reconcile through the existing session; do not blindly replay the input.
    Unknown,
}
