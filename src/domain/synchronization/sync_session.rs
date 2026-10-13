//! Identity-bearing progress snapshots of a local/target file-sync relationship.

use crate::{checkpoint::*, domain::workspace::*};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Durable state of one file-synchronization relationship.
///
/// This is an identity-bearing workflow entity: `id` remains stable while its
/// baseline, pending review, and publication receipt advance. [`crate::SyncService`]
/// owns those transitions under a repository lease. A copy is only a persisted
/// snapshot, not a second controller authorized to mutate the workflow.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncSession {
    /// Stable identity of this synchronization workflow.
    pub id: OperationId,
    /// Target workspace paired with the local project.
    pub workspace: WorkspaceRef,
    /// Canonical local project path; the caller must keep writers quiescent during publication.
    pub local_root: PathBuf,
    /// Common ancestor used for the next three-way review.
    ///
    /// Initially this is the restored starting snapshot. After publication it
    /// advances to the captured remote snapshot, preserving local-only changes
    /// as local changes during the next review.
    pub baseline: CheckpointRef,
    /// Latest staged three-way review, if publication is pending.
    pub latest_reconciliation: Option<ReconciliationRef>,
    /// Remote checkpoint associated with the pending review.
    pub pending_remote: Option<CheckpointRef>,
    /// Whether the baseline has been successfully restored to the target.
    pub initialized: bool,
    /// Most recent publication receipt used to recognize retries.
    pub last_apply: Option<ApplyReceipt>,
}
