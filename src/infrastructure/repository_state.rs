use crate::domain::RecoverySession;
use crate::{
    BackendDescriptor, Error, HandoffReceipt, OperationId, Result, SyncSession, WorkflowRecord,
    WorkspaceId,
};
use serde::{de::Error as DeserializeError, Deserializer};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::{mem::discriminant, result::Result as StdResult};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RepositoryState {
    pub(super) version: u32,
    pub(super) reservations: BTreeMap<WorkspaceId, String>,
    pub(super) workspaces: BTreeMap<WorkspaceId, BackendDescriptor>,
    #[serde(deserialize_with = "decode_workflows")]
    pub(super) workflows: BTreeMap<OperationId, WorkflowRecord>,
}
impl Default for RepositoryState {
    fn default() -> Self {
        Self {
            version: 3,
            reservations: BTreeMap::new(),
            workspaces: BTreeMap::new(),
            workflows: BTreeMap::new(),
        }
    }
}
impl RepositoryState {
    pub(super) fn save_workflow(
        &mut self,
        id: &OperationId,
        receipt: &WorkflowRecord,
    ) -> Result<()> {
        if matches!(receipt, WorkflowRecord::Recovery(_)) {
            return Err(Error::Conflict(
                "recovery authority requires compare-exchange".into(),
            ));
        }
        if id != receipt.id() {
            return Err(Error::InvalidInput(
                "workflow key must match the receipt identity".into(),
            ));
        }
        if let Some(previous) = self.workflows.get(id) {
            if discriminant(previous) != discriminant(receipt) {
                return Err(Error::Conflict(
                    "workflow ID cannot change its domain kind".into(),
                ));
            }
        }
        self.workflows.insert(id.clone(), receipt.clone());
        Ok(())
    }
    pub(super) fn load_recovery(&self, id: &OperationId) -> Result<Option<RecoverySession>> {
        self.workflows
            .get(id)
            .cloned()
            .map(WorkflowRecord::into_recovery)
            .transpose()
    }

    pub(super) fn compare_exchange_recovery(
        &mut self,
        id: &OperationId,
        expected_revision: Option<u64>,
        replacement: &RecoverySession,
    ) -> Result<()> {
        if id != &replacement.id {
            return Err(Error::InvalidInput(
                "recovery key must match session identity".into(),
            ));
        }
        let previous = self.load_recovery(id)?;
        if previous.as_ref().map(|session| session.revision) != expected_revision {
            return Err(Error::Conflict(
                "recovery revision no longer matches".into(),
            ));
        }
        replacement.validate_replacement(previous.as_ref())?;
        self.workflows
            .insert(id.clone(), WorkflowRecord::Recovery(replacement.clone()));
        Ok(())
    }

    pub(super) fn reserve(&mut self, id: &WorkspaceId, identity: &str) -> Result<()> {
        if let Some(previous) = self.reservations.get(id) {
            if previous != identity {
                return Err(Error::Conflict(format!(
                    "workspace {id} already has a different creation request"
                )));
            }
        } else {
            if self.workspaces.contains_key(id) {
                return Err(Error::Conflict(format!(
                    "workspace {id} was connected externally; reconnect it by reference"
                )));
            }
            self.reservations.insert(id.clone(), identity.to_owned());
        }
        Ok(())
    }
    pub(super) fn save_workspace(&mut self, descriptor: &BackendDescriptor) -> Result<()> {
        let id = &descriptor.reference.id;
        if let Some(previous) = self.workspaces.get(id) {
            if previous != descriptor {
                return Err(Error::Conflict(format!(
                    "workspace {id} cannot be rebound to a different target"
                )));
            }
        }
        self.workspaces.insert(id.clone(), descriptor.clone());
        Ok(())
    }
}

// Version 1 stored untagged, but still known, sync/handoff structures. Decode
// those explicitly; never discard an unreadable receipt or guess from JSON keys.
#[derive(Deserialize)]
#[serde(untagged)]
enum StoredWorkflow {
    Typed(WorkflowRecord),
    LegacySync(SyncSession),
    LegacyHandoff(HandoffReceipt),
}

fn decode_workflows<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> StdResult<BTreeMap<OperationId, WorkflowRecord>, D::Error> {
    let stored = BTreeMap::<OperationId, StoredWorkflow>::deserialize(deserializer)?;
    stored
        .into_iter()
        .map(|(id, value)| {
            let record = match value {
                StoredWorkflow::Typed(value) => value,
                StoredWorkflow::LegacySync(value) => WorkflowRecord::Sync(value),
                StoredWorkflow::LegacyHandoff(value) => WorkflowRecord::Handoff(value),
            };
            if &id != record.id() {
                return Err(DeserializeError::custom(
                    "workflow key differs from its receipt identity",
                ));
            }
            if let WorkflowRecord::Recovery(session) = &record {
                session.validate().map_err(DeserializeError::custom)?;
            }
            Ok((id, record))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        checkpoint::CheckpointRef, HandoffPhase, ProviderId, RunId, RunRef, WorkspaceLocator,
        WorkspaceRef,
    };
    use std::path::PathBuf;

    #[derive(Serialize)]
    struct TestLocator {
        target: &'static str,
    }

    fn session() -> SyncSession {
        SyncSession {
            id: OperationId::new("sync").unwrap(),
            workspace: WorkspaceRef {
                version: 1,
                id: WorkspaceId::new("workspace").unwrap(),
                provider: ProviderId::new("test").unwrap(),
                locator: WorkspaceLocator::from_locator(&TestLocator { target: "fixture" })
                    .unwrap(),
            },
            local_root: PathBuf::from("/test/project"),
            baseline: CheckpointRef("checkpoint".into()),
            latest_reconciliation: None,
            pending_remote: None,
            initialized: true,
            last_apply: None,
        }
    }

    #[test]
    fn workflow_key_and_kind_cannot_be_rebound() {
        let mut state = RepositoryState::default();
        let session = session();
        let id = session.id.clone();
        assert!(state
            .save_workflow(&OperationId::new("other").unwrap(), &session.clone().into())
            .is_err());
        state.save_workflow(&id, &session.clone().into()).unwrap();
        let handoff = HandoffReceipt {
            id: id.clone(),
            identity: "inputs".into(),
            source: RunRef {
                workspace: session.workspace.clone(),
                id: RunId::new("source").unwrap(),
            },
            destination: session.workspace,
            checkpoint: session.baseline,
            phase: HandoffPhase::Captured,
            destination_run: None,
        };
        assert!(matches!(
            state.save_workflow(&id, &handoff.into()),
            Err(Error::Conflict(_))
        ));
    }

    #[derive(Serialize)]
    struct LegacyState {
        version: u32,
        reservations: BTreeMap<WorkspaceId, String>,
        workspaces: BTreeMap<WorkspaceId, BackendDescriptor>,
        workflows: BTreeMap<OperationId, SyncSession>,
    }

    #[test]
    fn known_legacy_workflows_decode_without_losing_progress() {
        let session = session();
        let id = session.id.clone();
        let legacy = LegacyState {
            version: 1,
            reservations: BTreeMap::new(),
            workspaces: BTreeMap::new(),
            workflows: BTreeMap::from([(id.clone(), session)]),
        };
        let loaded: RepositoryState =
            serde_json::from_slice(&serde_json::to_vec(&legacy).unwrap()).unwrap();
        let loaded = loaded
            .workflows
            .get(&id)
            .unwrap()
            .clone()
            .into_sync()
            .unwrap();
        assert!(loaded.initialized);
        assert_eq!(loaded.baseline.0, "checkpoint");
        assert_eq!(loaded.id, id);
    }

    #[test]
    fn legacy_receipt_with_wrong_identity_is_rejected() {
        let legacy = LegacyState {
            version: 1,
            reservations: BTreeMap::new(),
            workspaces: BTreeMap::new(),
            workflows: BTreeMap::from([(OperationId::new("wrong").unwrap(), session())]),
        };
        assert!(
            serde_json::from_slice::<RepositoryState>(&serde_json::to_vec(&legacy).unwrap())
                .is_err()
        );
    }
}
