//! Lifecycle operation identities and deletion progress snapshots.
use super::WorkspaceRef;
use crate::domain::identifier::identifier;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

identifier!(
    /// Stable identity of a lifecycle or synchronization operation.
    ///
    /// An [`OperationRef`] adds workspace context for provider operations. Sync and
    /// handoff receipts also use this type as their durable workflow key.
    OperationId
);

#[cfg(test)]
mod operation_id_tests {
    use super::*;
    #[test]
    fn validates_direct_and_deserialized_ids() {
        for value in ["", ".", "..", "../unsafe", "space name"] {
            assert!(OperationId::new(value).is_err());
            assert!(serde_json::from_value::<OperationId>(serde_json::json!(value)).is_err());
        }
        assert_eq!(
            OperationId::new("valid-id_1").unwrap().as_str(),
            "valid-id_1"
        );
    }
}

/// A workspace-qualified handle for a provider lifecycle operation.
///
/// An [`OperationId`] alone cannot identify which provider and workspace to poll.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperationRef {
    /// Workspace whose resources the operation affects.
    pub workspace: WorkspaceRef,
    /// Provider operation identity used when polling its progress.
    pub id: OperationId,
}

/// A deletion receipt that can be persisted and polled after reconnecting.
///
/// An accepted request is not proof that resources have been removed. Only
/// [`DeleteState::Completed`] confirms completion reported by the provider.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeleteOperation {
    /// Handle used to inspect this deletion operation again.
    pub reference: OperationRef,
    /// Latest provider-reported progress.
    pub state: DeleteState,
    /// Optional provider explanation, such as a reason cleanup is blocked.
    pub detail: Option<String>,
}

/// Observed progress of an asynchronous resource deletion.
///
/// Keep the operation receipt while deletion is pending, blocked, or unknown.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeleteState {
    /// Deletion has been accepted and is awaiting processing.
    Pending,
    /// The provider is performing deletion.
    Processing,
    /// Deletion cannot currently complete; inspect the receipt detail.
    Blocked,
    /// The provider reports that deletion has completed.
    Completed,
    /// The provider cannot give a recognized, reliable deletion state.
    Unknown,
}
