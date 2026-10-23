use crate::{ports::IWorkflowLease, Result};
use std::path::Path;

/// Cooperative coordination for one local destination, injected into edit sessions.
/// This is not authorization or an OS lock against editors. All participating SDK
/// controllers must use the same canonical root and shared lease repository. The
/// caller must additionally stop unmanaged writers throughout begin/review/apply.
pub trait IEditCoordinator: Send + Sync {
    /// Canonical destination that this coordinator was constructed to protect.
    fn root(&self) -> &Path;
    /// Acquire a non-expiring, nonblocking cooperative claim; reject a replaced root.
    /// Keep the returned lease alive until the whole blocking operation completes.
    fn acquire(&self) -> Result<Box<dyn IWorkflowLease>>;
}
