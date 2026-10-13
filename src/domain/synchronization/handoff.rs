//! Durable phases and receipts for continuing a stopped run in another workspace.

use crate::{checkpoint::CheckpointRef, domain::*};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Last completed phase of a resumable handoff workflow.
///
/// Phases are progress snapshots, not commands to repeat all earlier work.
pub enum HandoffPhase {
    /// Source files were captured after confirming the source run had stopped.
    Captured,
    /// The captured files were restored to the destination.
    Restored,
    /// The continuation run was started and its reference was recorded.
    Started,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
/// Durable progress of one identity-bearing handoff operation.
///
/// The operation keeps the same `id` through capture, restore, and launch.
/// [`crate::SyncService`] validates its input fingerprint on retry, requires a
/// stopped source, and resumes from the recorded phase. This transfers selected
/// files, not process memory or an implicit agent conversation.
#[serde(deny_unknown_fields)]
pub struct HandoffReceipt {
    /// Stable identity of this handoff workflow.
    pub id: OperationId,
    /// Digest of the handoff inputs used to reject incompatible retries.
    pub identity: String,
    /// Confirmed-stopped source process.
    pub source: RunRef,
    /// Workspace receiving the continuation.
    pub destination: WorkspaceRef,
    /// Immutable artifact containing the selected source files.
    pub checkpoint: CheckpointRef,
    /// Last durably recorded completed phase.
    pub phase: HandoffPhase,
    /// Continuation process reference once launch has been recorded.
    pub destination_run: Option<RunRef>,
}
