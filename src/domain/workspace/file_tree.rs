//! Typed file-tree transfer values shared across the workspace and sync contexts.
use super::{OperationId, WorkspacePath};
use serde::{Deserialize, Serialize};

/// Value object identifying one temporary, SDK-owned file-tree transfer.
///
/// A transfer is not a checkpoint: it names transport staging files that can be
/// removed after upload/download. The archive path is derived from a validated
/// identity so callers cannot use cleanup to remove an arbitrary project path.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeTransfer {
    /// Stable identity of this temporary transfer.
    pub id: OperationId,
}

impl TreeTransfer {
    /// Allocate a fresh transfer identity.
    pub fn new() -> Self {
        Self {
            id: OperationId::generate(),
        }
    }

    /// Workspace-relative staging directory owned by this transfer.
    pub fn relative_directory(&self) -> WorkspacePath {
        WorkspacePath::new(format!(".workspace-sdk/transfers/{}", self.id))
            .expect("a validated operation identity produces a relative path")
    }

    /// Workspace-relative file containing the encoded tree archive.
    pub fn relative_archive_path(&self) -> WorkspacePath {
        WorkspacePath::new(format!("{}/tree.json", self.relative_directory().as_str()))
            .expect("a validated transfer directory produces a relative path")
    }
}

impl Default for TreeTransfer {
    fn default() -> Self {
        Self::new()
    }
}

/// Metadata returned after the target has exported a filtered file-tree archive.
///
/// Verify both byte count and digest after downloading before accepting the
/// archive into checkpoint storage. This is a result value, not mutable state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeExportReceipt {
    /// Exact length of the encoded archive, in bytes.
    pub bytes: u64,
    /// Lowercase hexadecimal SHA-256 digest of the encoded archive bytes.
    pub sha256: String,
    /// Number of selected file and symbolic-link entries.
    pub files: usize,
}

/// Result of restoring a file-tree archive to an empty or matching retry target.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeImportReceipt {
    /// Number of restored file and symbolic-link entries.
    pub files: usize,
}
