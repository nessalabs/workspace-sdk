//! Typed persistence boundary for synchronization workflow entities.
use super::{HandoffReceipt, RecoverySession, SyncSession};
use crate::{domain::workspace::OperationId, Error, Result};
use serde::{Deserialize, Serialize};

/// A persisted synchronization entity with an explicit workflow kind.
///
/// The repository stores domain records rather than unstructured JSON. JSON is
/// only an encoding choice inside the file repository. The enum also prevents
/// a sync-session ID from being silently interpreted as a handoff operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "receipt", rename_all = "snake_case")]
pub enum WorkflowRecord {
    /// Current state of a local/remote synchronization relationship.
    Sync(SyncSession),
    /// Current state of a stopped-source continuation handoff.
    Handoff(HandoffReceipt),
    /// Canonical revision-fenced placement and recovery state.
    Recovery(RecoverySession),
}

impl WorkflowRecord {
    /// Stable entity identity used as the repository key.
    pub fn id(&self) -> &OperationId {
        match self {
            Self::Sync(value) => &value.id,
            Self::Handoff(value) => &value.id,
            Self::Recovery(value) => &value.id,
        }
    }

    /// Extract a sync session, rejecting an ID reserved by another workflow kind.
    pub fn into_sync(self) -> Result<SyncSession> {
        match self {
            Self::Sync(value) => Ok(value),
            Self::Handoff(_) => Err(Error::Conflict(
                "operation ID belongs to a handoff, not a sync session".into(),
            )),
            Self::Recovery(_) => Err(Error::Conflict(
                "operation ID belongs to recovery, not a sync session".into(),
            )),
        }
    }

    /// Extract a handoff, rejecting an ID reserved by another workflow kind.
    pub fn into_handoff(self) -> Result<HandoffReceipt> {
        match self {
            Self::Handoff(value) => Ok(value),
            Self::Sync(_) => Err(Error::Conflict(
                "operation ID belongs to a sync session, not a handoff".into(),
            )),
            Self::Recovery(_) => Err(Error::Conflict(
                "operation ID belongs to recovery, not a handoff".into(),
            )),
        }
    }

    /// Extract recovery authority, rejecting identities reserved by other workflow kinds.
    pub fn into_recovery(self) -> Result<RecoverySession> {
        match self {
            Self::Recovery(value) => {
                value.validate()?;
                Ok(value)
            }
            _ => Err(Error::Conflict(
                "operation ID belongs to another workflow kind, not recovery".into(),
            )),
        }
    }
}

impl From<SyncSession> for WorkflowRecord {
    fn from(value: SyncSession) -> Self {
        Self::Sync(value)
    }
}

impl From<HandoffReceipt> for WorkflowRecord {
    fn from(value: HandoffReceipt) -> Self {
        Self::Handoff(value)
    }
}

impl From<RecoverySession> for WorkflowRecord {
    fn from(value: RecoverySession) -> Self {
        Self::Recovery(value)
    }
}
