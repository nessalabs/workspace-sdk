//! Completion values for materializing checkpoints or file-tree envelopes.
//!
//! Receipts describe a past restoration. They are not live directory handles or
//! mutable synchronization sessions.

use super::CheckpointRef;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Result of materializing an existing checkpoint into an empty local directory.
///
/// [`crate::checkpoint::ICheckpointEngine::restore`] refuses populated destinations.
/// For an existing working copy, use reconciliation and explicit apply instead.
/// This receipt records the restored selection, not a live synchronization link.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RestoreReceipt {
    /// Identity of the immutable snapshot that was materialized.
    pub checkpoint: CheckpointRef,
    /// Canonical local directory containing the restored files.
    pub destination: PathBuf,
    /// Selected-tree fingerprint of the materialized snapshot.
    ///
    /// Later edits do not update this historical value.
    pub fingerprint: String,
}

/// Result of validating and materializing a [`super::TreeArchive`] locally.
///
/// Archive restoration requires an empty real directory and validates all entry
/// paths, hashes, modes, sizes, and symlink targets before writing selected files.
/// It does not create a stored checkpoint identity; use
/// [`crate::checkpoint::ICheckpointEngine::capture_archive`] when one is needed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RestoreTreeReceipt {
    /// Canonical local directory populated with the archive's selected entries.
    pub destination: PathBuf,
    /// SHA-256 fingerprint of the sorted decoded entry metadata.
    pub fingerprint: String,
    /// Number of restored file and symlink entries; inferred directories are not counted.
    pub files: usize,
}
