use super::IWorkflowLease;
use crate::{domain::*, Result};

/// Durable, non-secret workflow receipts. A workflow service is controlled by one
/// controller; implementations must atomically replace each individual receipt.
pub trait IWorkflowRepository: Send + Sync {
    /// Acquire a nonblocking exclusive lease for this workflow. Keep the lease
    /// alive across the entire use case, including remote side effects.
    fn acquire(&self, id: &OperationId) -> Result<Box<dyn IWorkflowLease>>;
    /// Load a typed synchronization or handoff record by stable workflow identity.
    fn load(&self, id: &OperationId) -> Result<Option<WorkflowRecord>>;
    /// Atomically persist progress; reject a mismatching key or a change of workflow kind.
    fn save(&self, id: &OperationId, receipt: &WorkflowRecord) -> Result<()>;
}
