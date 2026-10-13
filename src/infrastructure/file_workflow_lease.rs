use crate::ports::IWorkflowLease;
use std::fs::File;

/// Closing the file releases its process-level flock.
pub(super) struct FileWorkflowLease(pub(super) File);
impl IWorkflowLease for FileWorkflowLease {}
impl Drop for FileWorkflowLease {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
    }
}
