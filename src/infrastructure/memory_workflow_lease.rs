use crate::{ports::IWorkflowLease, OperationId};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

pub(super) struct MemoryWorkflowLease {
    pub(super) leases: Arc<Mutex<BTreeSet<OperationId>>>,
    pub(super) id: OperationId,
}
impl IWorkflowLease for MemoryWorkflowLease {}
impl Drop for MemoryWorkflowLease {
    fn drop(&mut self) {
        if let Ok(mut leases) = self.leases.lock() {
            leases.remove(&self.id);
        }
    }
}
