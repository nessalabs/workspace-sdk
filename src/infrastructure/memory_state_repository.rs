use super::repository_state::RepositoryState;
use crate::{ports::IWorkflowLease, WorkflowRecord};
use crate::{
    ports::IWorkflowRepository, BackendDescriptor, Error, IWorkspaceRepository, OperationId,
    Result, WorkspaceId,
};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

#[derive(Default)]
pub struct MemoryStateRepository {
    state: Mutex<RepositoryState>,
    leases: Arc<Mutex<BTreeSet<OperationId>>>,
}
impl IWorkspaceRepository for MemoryStateRepository {
    fn reserve(&self, id: &WorkspaceId, identity: &str) -> Result<()> {
        self.state.lock().map_err(poisoned)?.reserve(id, identity)
    }
    fn save(&self, descriptor: &BackendDescriptor) -> Result<()> {
        self.state
            .lock()
            .map_err(poisoned)?
            .save_workspace(descriptor)
    }
    fn get(&self, id: &WorkspaceId) -> Result<Option<BackendDescriptor>> {
        Ok(self
            .state
            .lock()
            .map_err(poisoned)?
            .workspaces
            .get(id)
            .cloned())
    }
    fn list(&self) -> Result<Vec<BackendDescriptor>> {
        Ok(self
            .state
            .lock()
            .map_err(poisoned)?
            .workspaces
            .values()
            .cloned()
            .collect())
    }
}
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_id_cannot_change_request_identity() {
        let repository = MemoryStateRepository::default();
        let id = WorkspaceId::new("one").unwrap();
        repository.reserve(&id, "a").unwrap();
        assert!(matches!(
            repository.reserve(&id, "b"),
            Err(Error::Conflict(_))
        ));
    }
}
