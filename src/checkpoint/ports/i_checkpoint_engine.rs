use crate::checkpoint::domain::*;
use crate::Result;
use std::path::Path;

/// Inject this port into application workflows; transports move the bundle bytes.
pub trait ICheckpointEngine: Send + Sync {
    /// Capture selected files at a caller-established quiescent boundary.
    ///
    /// Mandatory exclusions always apply; untracked files also obey Git ignore
    /// rules. The result is immutable and does not retain repository history.
    fn capture(&self, root: &Path, options: CaptureOptions) -> Result<Checkpoint>;
    /// Export a standalone snapshot bundle and its validated manifest.
    fn export(&self, checkpoint: &CheckpointRef) -> Result<CheckpointBundle>;
    /// Validate and import a snapshot bundle without trusting its Git metadata.
    fn import(&self, bundle: &CheckpointBundle) -> Result<Checkpoint>;
    /// Materialize a snapshot into an empty, real directory.
    fn restore(&self, checkpoint: &CheckpointRef, destination: &Path) -> Result<RestoreReceipt>;
    /// Stage a merge, preserving all three input trees. Never writes into `local`.
    fn reconcile(
        &self,
        base: &CheckpointRef,
        local: &Path,
        remote: &CheckpointRef,
    ) -> Result<Reconciliation>;
    /// Accept the caller's manually resolved review directory as a new snapshot.
    /// `resolved_paths` must explicitly acknowledge every reported conflict.
    fn accept_resolution(
        &self,
        reconciliation: &ReconciliationRef,
        resolved_paths: &[String],
    ) -> Result<Reconciliation>;
    /// Publish the reviewed tree after checking quiescence and the expected
    /// destination fingerprint. Retains a durable backup for explicit rollback.
    fn apply(
        &self,
        reconciliation: &ReconciliationRef,
        options: ApplyOptions,
    ) -> Result<ApplyReceipt>;
    /// Read authoritative apply progress before replacing a pending sync review.
    fn inspect_apply(&self, reconciliation: &ReconciliationRef) -> Result<ApplyState>;
    /// Recover an interrupted/failed apply using its durable backup. Requires the
    /// same exclusive-access assertion as apply and preserves the recovery data.
    fn rollback(&self, recovery: &RecoveryRef, quiescent: bool) -> Result<()>;
    /// Portable transport envelope for a target helper; uses the same filters.
    fn export_tree_archive(&self, root: &Path) -> Result<Vec<u8>>;
    /// Validate the entire archive before materializing into an empty directory.
    fn import_tree_archive(&self, bytes: &[u8], destination: &Path) -> Result<RestoreTreeReceipt>;
    /// Export an already selected snapshot without reapplying working-tree ignores.
    fn export_checkpoint_archive(&self, checkpoint: &CheckpointRef) -> Result<Vec<u8>>;
    /// Capture a validated target archive without losing tracked-but-ignored files.
    fn capture_archive(&self, bytes: &[u8], options: CaptureOptions) -> Result<Checkpoint>;
}
