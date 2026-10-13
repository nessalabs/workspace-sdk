//! Apply precondition values, completion receipts, and recovery-progress snapshots.
//!
//! References identify retained recovery records. Receipts are historical values;
//! the engine's journal is authoritative for an operation's current recovery state.

use super::{CheckpointRef, ReconciliationRef};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Preconditions supplied when applying a staged reconciliation to local files.
///
/// Applying is an explicit operation: a clean merge alone never permits writes.
/// The Git engine validates both fields against its persisted plan on initial
/// calls and idempotent retries. Fingerprint checks cannot lock an editor.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApplyOptions {
    /// Caller assertion that all writers are stopped and exclusive access is held.
    ///
    /// This must be `true`. The SDK cannot establish quiescence merely from this
    /// flag, so callers must stop editors, agents, and other independent writers.
    pub quiescent: bool,
    /// Original local-selection fingerprint from [`super::Reconciliation::expected_fingerprint`].
    ///
    /// Do not substitute the merged result's fingerprint. A first apply also
    /// verifies that the destination still matches the captured local selection.
    pub expected_fingerprint: String,
}

/// Durable result of successfully applying one reconciliation.
///
/// Retain this to identify the resulting snapshot and recovery data. Repeating
/// the same apply returns its prior receipt without overwriting later edits;
/// therefore a receipt describes an operation's completion, not current files.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApplyReceipt {
    /// Reconciliation whose reviewed result was applied.
    pub reconciliation: ReconciliationRef,
    /// Immutable snapshot of the applied merged/resolved selection.
    pub checkpoint: CheckpointRef,
    /// Recovery journal retaining references to the before and after snapshots.
    ///
    /// Rollback requires separate explicit authorization through the engine's
    /// quiescent argument; retaining this identifier does not perform rollback.
    pub recovery: RecoveryRef,
    /// Selected-tree SHA-256 fingerprint recorded for the successfully applied result.
    ///
    /// It may differ from today's destination if the user has edited or rolled back later.
    pub fingerprint: String,
}

/// Authoritative durable apply progress returned by read-only engine inspection.
///
/// Inspection reads the recovery journal without changing files or guessing that
/// an interrupted publication finished. Retry the original apply to recover a
/// completed publication, or explicitly roll back its recovery record.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ApplyState {
    /// No apply intent has been persisted for this review, which may still have conflicts.
    NotStarted,
    /// An apply intent exists but its journal has no recorded completion.
    ///
    /// Files may be unchanged, partly updated, or fully published. Inspection
    /// intentionally does not infer success from the current tree.
    Interrupted {
        /// Durable recovery identity needed to retry or roll back this attempt.
        recovery: RecoveryRef,
    },
    /// The recovery journal records restoration of the selected before snapshot.
    ///
    /// Stage a new reconciliation before attempting another apply.
    RolledBack {
        /// Identity of the recovery journal that completed the rollback.
        recovery: RecoveryRef,
    },
    /// The journal records successful publication of the reviewed selection.
    Applied {
        /// Exact operation receipt; it does not assert that no later edits occurred.
        receipt: ApplyReceipt,
    },
}

/// Opaque identity of retained before/after snapshots and an apply recovery journal.
///
/// Obtain it from an [`ApplyReceipt`], [`ApplyState`], or the reconciliation's
/// recovery field. The Git engine validates identifiers on use as nonempty,
/// at most 80 bytes, and ASCII letters/digits/hyphens only. Construction and
/// transparent-string deserialization do not themselves validate this wrapper.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RecoveryRef(
    /// Engine-local recovery identifier; never a filesystem path or project-relative name.
    pub String,
);

/// Internal, versioned record of a single apply attempt and its rollback inputs.
///
/// Persisted before mutation; public callers inspect it through [`ApplyState`]
/// rather than interpreting this storage representation directly.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct RecoveryJournal {
    /// Storage schema version, currently one.
    pub version: u32,
    /// Stable identity of this recovery attempt.
    pub reference: RecoveryRef,
    /// Canonical local directory affected by apply or rollback.
    pub destination: PathBuf,
    /// Immutable selected snapshot to restore during rollback.
    pub before: CheckpointRef,
    /// Immutable selected snapshot intended for publication.
    pub after: CheckpointRef,
    /// Durable phase: `applying`, `applied`, or `rolled_back`.
    pub status: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_apply_requests_must_state_quiescence_and_expected_contents() {
        assert!(serde_json::from_str::<ApplyOptions>(r#"{"expected_fingerprint":"abc"}"#).is_err());
        assert!(serde_json::from_str::<ApplyOptions>(r#"{"quiescent":true}"#).is_err());
        let options: ApplyOptions =
            serde_json::from_str(r#"{"quiescent":false,"expected_fingerprint":"abc"}"#).unwrap();
        assert!(!options.quiescent);
    }

    #[test]
    fn interrupted_progress_retains_its_recovery_identity_in_saved_state() {
        let state: ApplyState =
            serde_json::from_str(r#"{"status":"interrupted","recovery":"recovery-123"}"#).unwrap();
        assert!(
            matches!(state, ApplyState::Interrupted { recovery } if recovery.0 == "recovery-123")
        );
        assert!(serde_json::from_str::<ApplyState>(r#"{"status":"interrupted"}"#).is_err());
    }
}
