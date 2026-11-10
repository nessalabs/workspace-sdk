use crate::domain::ProviderId;
use crate::domain::{MachineRecord, MachineState, ProvisionKey, RecoverySession, RequestOrigin};
use crate::{
    BackendDescriptor, Error, HandoffReceipt, OperationId, Result, SyncSession, WorkflowRecord,
    WorkspaceId,
};
use serde::{de::Error as DeserializeError, Deserializer};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::{mem::discriminant, result::Result as StdResult};

pub(super) const VERSION: u32 = 4;

#[derive(Serialize)]
pub(super) struct RepositoryState {
    pub(super) version: u32,
    pub(super) machines: BTreeMap<WorkspaceId, MachineRecord>,
    /// The last revision of each forgotten record, so a new record for the
    /// same ID never reuses a revision an in-flight caller still holds.
    pub(super) retired: BTreeMap<WorkspaceId, u64>,
    pub(super) workflows: BTreeMap<OperationId, WorkflowRecord>,
}
impl Default for RepositoryState {
    fn default() -> Self {
        Self {
            version: VERSION,
            machines: BTreeMap::new(),
            retired: BTreeMap::new(),
            workflows: BTreeMap::new(),
        }
    }
}

/// Every stored version. Versions 1 to 3 kept a request reservation and a
/// descriptor in two maps; version 4 keeps one machine record per ID.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredState {
    version: u32,
    #[serde(default)]
    reservations: Option<BTreeMap<WorkspaceId, String>>,
    #[serde(default)]
    workspaces: Option<BTreeMap<WorkspaceId, BackendDescriptor>>,
    #[serde(default)]
    machines: Option<BTreeMap<WorkspaceId, MachineRecord>>,
    #[serde(default)]
    retired: Option<BTreeMap<WorkspaceId, u64>>,
    #[serde(deserialize_with = "decode_workflows")]
    workflows: BTreeMap<OperationId, WorkflowRecord>,
}
impl<'de> Deserialize<'de> for RepositoryState {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> StdResult<Self, D::Error> {
        let stored = StoredState::deserialize(deserializer)?;
        let machines = match stored.version {
            1..=3 => {
                if stored.machines.is_some() || stored.retired.is_some() {
                    return Err(DeserializeError::custom("machines field in an old state"));
                }
                let machines = upgrade(
                    stored.reservations.unwrap_or_default(),
                    stored.workspaces.unwrap_or_default(),
                )
                .map_err(DeserializeError::custom)?;
                for record in machines.values() {
                    record.validate(None).map_err(DeserializeError::custom)?;
                }
                machines
            }
            VERSION => {
                if stored.reservations.is_some() || stored.workspaces.is_some() {
                    return Err(DeserializeError::custom("old fields in a version 4 state"));
                }
                let machines = stored.machines.unwrap_or_default();
                let retired = stored.retired.as_ref();
                for (id, record) in &machines {
                    if id != &record.workspace_id {
                        return Err(DeserializeError::custom(
                            "machine key differs from its record identity",
                        ));
                    }
                    record
                        .validate(retired.and_then(|retired| retired.get(id)).copied())
                        .map_err(DeserializeError::custom)?;
                }
                machines
            }
            // Rejected by the caller with a typed Unsupported error.
            _ => BTreeMap::new(),
        };
        Ok(Self {
            version: stored.version,
            machines,
            retired: stored.retired.unwrap_or_default(),
            workflows: stored.workflows,
        })
    }
}

/// Old request hashes cannot be compared with new requests (option shapes
/// changed), so every upgraded record is `Upgraded`: the first create or attach
/// that fits it adopts it. A saved descriptor becomes Ready. A reservation with
/// no descriptor never finished; it may have been a create or an attach, so it
/// becomes Provisioning under the key a create was given, and either kind of
/// request can take it over.
fn upgrade(
    reservations: BTreeMap<WorkspaceId, String>,
    workspaces: BTreeMap<WorkspaceId, BackendDescriptor>,
) -> Result<BTreeMap<WorkspaceId, MachineRecord>> {
    let mut machines = BTreeMap::new();
    for (id, descriptor) in workspaces {
        if descriptor.reference.id != id {
            return Err(Error::State(
                "workspace key differs from its descriptor".into(),
            ));
        }
        let origin = if reservations.contains_key(&id) {
            RequestOrigin::Upgraded
        } else {
            RequestOrigin::Connected
        };
        machines.insert(
            id.clone(),
            MachineRecord {
                workspace_id: id,
                origin,
                revision: 1,
                generation: 1,
                state: MachineState::Ready { descriptor },
            },
        );
    }
    for id in reservations.into_keys() {
        machines.entry(id.clone()).or_insert_with(|| MachineRecord {
            state: MachineState::Provisioning {
                key: ProvisionKey::for_workspace(&id),
            },
            workspace_id: id,
            origin: RequestOrigin::Upgraded,
            revision: 1,
            generation: 1,
        });
    }
    Ok(machines)
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

    pub(super) fn begin(
        &mut self,
        id: &WorkspaceId,
        provider: &ProviderId,
        request: &str,
        initial: MachineState,
    ) -> Result<MachineRecord> {
        let (record, changed) = MachineRecord::begin(
            self.machines.get(id),
            id,
            provider,
            request,
            initial,
            self.retired.get(id).copied(),
        )?;
        if changed {
            self.machines.insert(id.clone(), record.clone());
        }
        Ok(record)
    }
    pub(super) fn advance(
        &mut self,
        id: &WorkspaceId,
        expected_revision: u64,
        next: MachineState,
    ) -> Result<MachineRecord> {
        let previous = self
            .machines
            .get(id)
            .ok_or_else(|| Error::NotFound(format!("workspace {id}")))?;
        let record = previous.advance(expected_revision, next)?;
        self.machines.insert(id.clone(), record.clone());
        Ok(record)
    }
    pub(super) fn forget(&mut self, id: &WorkspaceId, expected_revision: u64) -> Result<()> {
        let previous = self
            .machines
            .get(id)
            .ok_or_else(|| Error::NotFound(format!("workspace {id}")))?;
        previous.check_forget(expected_revision)?;
        self.retired.insert(id.clone(), previous.revision);
        self.machines.remove(id);
        Ok(())
    }
    pub(super) fn save_workspace(&mut self, descriptor: &BackendDescriptor) -> Result<()> {
        let id = &descriptor.reference.id;
        let Some(previous) = self.machines.get(id) else {
            let record = MachineRecord::connected(descriptor, self.retired.get(id).copied());
            self.machines.insert(id.clone(), record);
            return Ok(());
        };
        if let Some(next) = previous.refresh(descriptor)? {
            self.machines.insert(id.clone(), next);
        }
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
