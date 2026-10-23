//! Immutable input values for isolated selected-tree editing, separate from live file observations.
use super::WorkspacePath;
use serde::{Deserialize, Serialize};

/// Session-qualified candidate revision. It is not a live-filesystem CAS token.
/// Obtain from a session and return unchanged with every read/stage/review/apply.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EditRevision(pub(crate) String);
impl EditRevision {
    /// Opaque transport spelling; tokens from another session are always stale.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// One whole-file candidate mutation. No variant writes the live destination.
/// Each request is checked against a complete candidate revision. Regular binary
/// file create/replace is bounded to 256 KiB. Rename never replaces its target.
#[derive(Clone)]
pub enum EditChange {
    /// Add a regular file only when its candidate path is absent.
    Create {
        /// Project-relative destination.
        path: WorkspacePath,
        /// Unencoded binary bytes, at most 256 KiB.
        bytes: Vec<u8>,
        /// Whether the candidate should be executable.
        executable: bool,
    },
    /// Replace one existing regular file, never follow a candidate symlink.
    Replace {
        /// Existing project-relative regular file.
        path: WorkspacePath,
        /// Complete replacement bytes, at most 256 KiB.
        bytes: Vec<u8>,
        /// Whether the candidate should be executable.
        executable: bool,
    },
    /// Remove one selected file or symlink from the candidate, without following it.
    Delete {
        /// Existing project-relative entry.
        path: WorkspacePath,
    },
    /// Move one selected entry to an absent path; directories are inferred.
    Rename {
        /// Existing selected file or symlink.
        from: WorkspacePath,
        /// Absent project-relative destination.
        to: WorkspacePath,
    },
}

impl EditChange {
    pub(crate) fn validate(&self) -> crate::Result<()> {
        let (first, second, bytes) = match self {
            Self::Create { path, bytes, .. } | Self::Replace { path, bytes, .. } => {
                (path, None, Some(bytes))
            }
            Self::Delete { path } => (path, None, None),
            Self::Rename { from, to } => (from, Some(to), None),
        };
        for path in std::iter::once(first).chain(second) {
            if path == &WorkspacePath::root() || path.as_str().len() > 4096 {
                return Err(crate::Error::InvalidInput(
                    "candidate mutation requires a non-root path of at most 4096 bytes".into(),
                ));
            }
        }
        if bytes.is_some_and(|bytes| bytes.len() > 262144) {
            return Err(crate::Error::InvalidInput(
                "staged whole-file writes are limited to 256 KiB".into(),
            ));
        }
        Ok(())
    }
}
