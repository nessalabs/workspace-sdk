use super::repository_state::RepositoryState;
use crate::domain::ProviderId;
#[cfg(feature = "portability")]
use crate::{
    domain::RecoverySession,
    ports::{IRecoveryRepository, IWorkflowLease, IWorkflowRepository},
    OperationId, WorkflowRecord,
};
use crate::{
    domain::{MachineRecord, MachineState},
    BackendDescriptor, Error, IWorkspaceRepository, Result, WorkspaceId,
};
use std::sync::Mutex;
#[cfg(feature = "portability")]
use std::{collections::BTreeSet, sync::Arc};

#[derive(Default)]
pub struct MemoryStateRepository {
    state: Mutex<RepositoryState>,
    #[cfg(feature = "portability")]
    leases: Arc<Mutex<BTreeSet<OperationId>>>,
}
impl IWorkspaceRepository for MemoryStateRepository {
    fn begin(
        &self,
        id: &WorkspaceId,
        provider: &ProviderId,
        request: &str,
        initial: MachineState,
    ) -> Result<MachineRecord> {
        self.state
            .lock()
            .map_err(poisoned)?
            .begin(id, provider, request, initial)
    }
    fn advance(
        &self,
        id: &WorkspaceId,
        expected_revision: u64,
        next: MachineState,
    ) -> Result<MachineRecord> {
        self.state
            .lock()
            .map_err(poisoned)?
            .advance(id, expected_revision, next)
    }
    fn forget(&self, id: &WorkspaceId, expected_revision: u64) -> Result<()> {
        self.state
            .lock()
            .map_err(poisoned)?
            .forget(id, expected_revision)
    }
    fn save(&self, descriptor: &BackendDescriptor) -> Result<()> {
        self.state
            .lock()
            .map_err(poisoned)?
            .save_workspace(descriptor)
    }
    fn record(&self, id: &WorkspaceId) -> Result<Option<MachineRecord>> {
        Ok(self
            .state
            .lock()
            .map_err(poisoned)?
            .machines
            .get(id)
            .cloned())
    }
    fn records(&self) -> Result<Vec<MachineRecord>> {
        Ok(self
            .state
            .lock()
            .map_err(poisoned)?
            .machines
            .values()
            .cloned()
            .collect())
    }
}
#[cfg(feature = "portability")]
impl IWorkflowRepository for MemoryStateRepository {
    fn acquire(&self, id: &OperationId) -> Result<Box<dyn IWorkflowLease>> {
        if !self.leases.lock().map_err(poisoned)?.insert(id.clone()) {
            return Err(Error::Conflict(format!(
                "workflow {id} is already controlled by another operation"
            )));
        }
        Ok(Box::new(
            super::memory_workflow_lease::MemoryWorkflowLease {
                leases: self.leases.clone(),
                id: id.clone(),
            },
        ))
    }
    fn load(&self, id: &OperationId) -> Result<Option<WorkflowRecord>> {
        Ok(self
            .state
            .lock()
            .map_err(poisoned)?
            .workflows
            .get(id)
            .cloned())
    }
    fn save(&self, id: &OperationId, receipt: &WorkflowRecord) -> Result<()> {
        self.state
            .lock()
            .map_err(poisoned)?
            .save_workflow(id, receipt)
    }
}
fn poisoned<T>(_: T) -> Error {
    Error::State("state repository lock poisoned".into())
}

#[cfg(feature = "portability")]
impl IRecoveryRepository for MemoryStateRepository {
    fn acquire_recovery(&self, id: &OperationId) -> Result<Box<dyn IWorkflowLease>> {
        IWorkflowRepository::acquire(self, id)
    }
    fn load_recovery(&self, id: &OperationId) -> Result<Option<RecoverySession>> {
        self.state.lock().map_err(poisoned)?.load_recovery(id)
    }
    fn compare_exchange_recovery(
        &self,
        id: &OperationId,
        expected_revision: Option<u64>,
        replacement: &RecoverySession,
    ) -> Result<()> {
        self.state
            .lock()
            .map_err(poisoned)?
            .compare_exchange_recovery(id, expected_revision, replacement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_id_cannot_change_request_identity() {
        let repository = MemoryStateRepository::default();
        let id = WorkspaceId::new("one").unwrap();
        let attaching = || MachineState::Attaching;
        let local = ProviderId::new("local").unwrap();
        repository.begin(&id, &local, "a", attaching()).unwrap();
        assert!(matches!(
            repository.begin(&id, &local, "b", attaching()),
            Err(Error::Conflict(_))
        ));
    }
}
