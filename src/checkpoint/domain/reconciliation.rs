//! Review-lifecycle snapshots, reference values, and preserved conflict evidence.
//!
//! The engine owns the persistent review lifecycle. These Rust values let callers
//! inspect it and name later operations without exposing mutable engine internals.

use super::{ApplyReceipt, CheckpointRef, RecoveryRef};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Opaque identity of a staged three-way merge and its durable local records.
///
/// The engine generates this reference and uses it to accept a resolution, apply
/// files, or inspect apply progress. It serializes as a plain string. The Git
/// engine validates nonempty identifiers of at most 80 ASCII letters, digits,
/// or hyphens on use; this public wrapper does not validate during construction.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReconciliationRef(
    /// Identifier within the engine's private reconciliation store, not a path.
    pub String,
);

/// Review/apply status recorded on a [`Reconciliation`].
///
/// Use [`crate::checkpoint::ICheckpointEngine::inspect_apply`] for authoritative
/// apply/recovery progress, particularly after interruption or explicit rollback.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconciliationStatus {
    /// A clean merge or explicitly accepted resolution is available for review.
    ///
    /// No destination write is implied; applying still requires quiescence and
    /// the expected destination fingerprint.
    StagedForReview,
    /// Git reported conflicts; the application must resolve and acknowledge them.
    Conflicted,
    /// A successful apply was recorded; this is not a promise that files remain unchanged.
    ///
    /// A later rollback is recorded in its recovery journal, so callers should
    /// inspect apply progress instead of interpreting this field in isolation.
    Applied,
}

/// Serializable snapshot of the review lifecycle for a three-way file reconciliation.
///
/// The persistent review has an identity and advances through staging, conflict
/// resolution, and apply. This type is its returned state snapshot; the engine
/// owns transitions, rather than exposing a mutable domain entity to callers.
///
/// Reconciliation never writes the destination. It creates a separate review
/// directory and retains all three originals. Users resolve conflicts there, then
/// explicitly accept the edited result before applying. Editing this returned
/// value does not alter the engine's persisted plan; operations use its reference.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reconciliation {
    /// Durable identity used for resolution, inspection, and apply operations.
    pub reference: ReconciliationRef,
    /// Common-ancestor snapshot against which both sides are compared.
    pub base: CheckpointRef,
    /// Snapshot of the local destination captured when this merge was staged.
    pub local: CheckpointRef,
    /// Snapshot of the remote result supplied to the merge.
    pub remote: CheckpointRef,
    /// Canonical local directory that an explicit apply would update.
    pub destination: PathBuf,
    /// Local directory containing merged files or editable conflict material.
    ///
    /// It contains no Git control directory. Changes must be accepted by the
    /// engine before they can replace an already captured result snapshot.
    pub review_directory: PathBuf,
    /// Local directory retaining the base/local/remote trees and exact conflict blobs.
    ///
    /// Keep the engine's state available while reviewing or recovering changes.
    pub originals_directory: PathBuf,
    /// Fingerprint of the captured local selection, required in [`super::ApplyOptions`].
    ///
    /// It detects stale selected contents; it neither covers excluded paths nor
    /// establishes exclusive access against an editor.
    pub expected_fingerprint: String,
    /// Immutable merged snapshot, present for clean or explicitly accepted results.
    pub result: Option<CheckpointRef>,
    /// Persisted review status; use apply inspection for recovery-journal progress.
    pub status: ReconciliationStatus,
    /// Original conflicts requiring explicit acknowledgement during resolution.
    ///
    /// Records remain available after acceptance as provenance for the reviewed change.
    pub conflicts: Vec<ConflictRecord>,
    /// Apply recovery identity, persisted before destination mutation begins.
    ///
    /// Its journal distinguishes interruption, completion, and rollback. An absent
    /// value normally means that no apply operation has started.
    #[serde(default)]
    pub recovery: Option<RecoveryRef>,
    /// Recorded successful apply receipt, reused by idempotent retries.
    ///
    /// This historical receipt can coexist with a later rollback journal; inspect
    /// authoritative apply state before advancing an application's sync baseline.
    #[serde(default)]
    pub applied: Option<ApplyReceipt>,
}

/// One path that Git could not reconcile automatically.
///
/// Each available side preserves exact bytes for the user's merge tool. A missing
/// side means that merge stage has no blob, for example after deletion or in a
/// file/directory conflict. Full original trees also remain available through
/// [`Reconciliation::originals_directory`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConflictRecord {
    /// Git's project-relative conflict path, including generated names when needed.
    ///
    /// File/directory conflicts can use a name such as `item~HEAD`; use each
    /// side's preserved path to obtain its actual bytes.
    pub path: String,
    /// Common-ancestor blob, absent when no stage-one blob exists.
    pub base: Option<ConflictSide>,
    /// Local blob, absent when no stage-two blob exists.
    pub local: Option<ConflictSide>,
    /// Remote blob, absent when no stage-three blob exists.
    pub remote: Option<ConflictSide>,
}

/// Preserved bytes and portable mode for one available side of a conflict.
///
/// This object exists only when that side has a Git blob; absence is represented
/// by `None` on [`ConflictRecord`]. Preserved blobs are ordinary local files even
/// for symlinks, whose blob bytes contain their relative target text.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConflictSide {
    /// Git object ID of this side's exact blob content.
    pub object_id: String,
    /// Original Git mode spelling: `100644`, `100755`, or `120000`.
    pub mode: u32,
    /// Absolute local path to the preserved raw blob bytes in private engine state.
    ///
    /// This path remains usable even when Git assigned a generated conflict name.
    pub preserved_path: PathBuf,
}
