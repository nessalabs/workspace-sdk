/// An exclusive workflow claim released when dropped. Implementations must not
/// expire while held; loss of the controller process releases native file locks.
pub trait IWorkflowLease: Send + Sync {}
