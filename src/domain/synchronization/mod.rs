//! Synchronization workflow context: durable progress and retry identity.
//!
//! [`SyncSession`] and [`HandoffReceipt`] have stable identities and changing
//! progress. They are persisted entity snapshots; [`crate::SyncService`] is the
//! application service that enforces transitions under an exclusive workflow
//! lease. Their checkpoint references point to immutable artifacts owned by
//! [`crate::checkpoint`]. The workspace and execution contexts own target and
//! process behavior; synchronization coordinates them without owning their state.

pub mod handoff;
pub mod recovery;
pub mod sync_session;
pub mod workflow_record;

pub use handoff::{HandoffPhase, HandoffReceipt};
pub use recovery::{
    RecoveryAttempt, RecoveryCandidate, RecoveryHead, RecoveryPhase, RecoveryRetainedWorkspace,
    RecoverySession,
};
pub use sync_session::SyncSession;
pub use workflow_record::WorkflowRecord;
