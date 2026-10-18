use super::git_checkpoint_engine::GitCheckpointEngine;
use crate::checkpoint::domain::*;
use crate::checkpoint::ports::ICheckpointEngine;
use crate::Result;
use std::path::Path;

impl ICheckpointEngine for GitCheckpointEngine {
    fn capture(&self, root: &Path, options: CaptureOptions) -> Result<Checkpoint> {
        self.capture_operation(root, options)
    }

    fn export(&self, reference: &CheckpointRef) -> Result<CheckpointBundle> {
        self.export_operation(reference)
    }

    fn import(&self, bundle: &CheckpointBundle) -> Result<Checkpoint> {
        self.import_operation(bundle)
    }

    fn restore(&self, reference: &CheckpointRef, destination: &Path) -> Result<RestoreReceipt> {
        self.restore_operation(reference, destination)
    }

    fn reconcile(
        &self,
        base: &CheckpointRef,
        local: &Path,
        remote: &CheckpointRef,
    ) -> Result<Reconciliation> {
        self.reconcile_operation(base, local, remote)
    }

    fn accept_resolution(
        &self,
        reference: &ReconciliationRef,
        resolved_paths: &[String],
    ) -> Result<Reconciliation> {
        self.accept_resolution_operation(reference, resolved_paths)
    }

    fn apply(&self, reference: &ReconciliationRef, options: ApplyOptions) -> Result<ApplyReceipt> {
        self.apply_operation(reference, options)
    }

    fn inspect_apply(&self, reference: &ReconciliationRef) -> Result<ApplyState> {
        self.inspect_apply_operation(reference)
    }

    fn rollback(&self, reference: &RecoveryRef, quiescent: bool) -> Result<()> {
        self.rollback_operation(reference, quiescent)
    }

    fn export_tree_archive(&self, root: &Path) -> Result<Vec<u8>> {
        self.export_tree_archive_operation(root)
    }

    fn import_tree_archive(&self, bytes: &[u8], destination: &Path) -> Result<RestoreTreeReceipt> {
        self.import_tree_archive_operation(bytes, destination)
    }

    fn export_checkpoint_archive(&self, reference: &CheckpointRef) -> Result<Vec<u8>> {
        self.export_checkpoint_archive_operation(reference)
    }

    fn fingerprint_archive(&self, bytes: &[u8]) -> Result<String> {
        self.fingerprint_archive_operation(bytes)
    }

    fn capture_archive(&self, bytes: &[u8], options: CaptureOptions) -> Result<Checkpoint> {
        self.capture_archive_operation(bytes, options)
    }
}
