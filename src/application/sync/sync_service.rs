//! File sync and resumable, single-controller handoff orchestration.
use super::{HandoffPhase, HandoffReceipt, SyncSession};
use crate::application::{Run, Workspace};
use crate::{checkpoint::*, domain::*, ports::IWorkflowRepository, Error, Result};
use sha2::{Digest, Sha256};
use std::{fs, future::Future};
use std::{path::Path, sync::Arc};
use tokio::{sync::Mutex, task::spawn_blocking};

const CHUNK: usize = 192 * 1024;
const MAX_ARCHIVE: usize = 380 * 1024 * 1024;

/// Shared sync policy is independent of provider APIs. Git runs on the
/// controller; targets use the Rust supervisor and Git for filtered snapshot export.
/// Keep the engine and receipts outside every synchronized project.
/// One controller owns each session/handoff. Quiescence assertions require the
/// caller to stop *all* writers, including editors and unmanaged processes.
/// Once an operation has acquired its workflow lease, an owned Tokio task keeps
/// it running through staging cleanup and receipt publication if its observer is
/// canceled. Keep asserted writers quiescent through completion. Initialize, sync-back,
/// and handoff retain workflow receipts; direct capture/restore have no pollable
/// workflow handle, so retain and await an application-owned task rather than
/// canceling its observer. Runtime shutdown or process death can still interrupt
/// work; cleanup transport failures remain best effort. Dropping a waiting observer
/// before admission does not start an operation.
#[derive(Clone)]
pub struct SyncService {
    checkpoints: Arc<dyn ICheckpointEngine>,
    receipts: Arc<dyn IWorkflowRepository>,
    operations: Arc<Mutex<()>>,
}
impl SyncService {
    pub fn new(
        checkpoints: Arc<dyn ICheckpointEngine>,
        receipts: Arc<dyn IWorkflowRepository>,
    ) -> Self {
        Self {
            checkpoints,
            receipts,
            operations: Arc::new(Mutex::new(())),
        }
    }
    pub fn checkpoints(&self) -> Arc<dyn ICheckpointEngine> {
        self.checkpoints.clone()
    }
    pub fn session(&self, id: &OperationId) -> Result<SyncSession> {
        self.receipts
            .load(id)?
            .ok_or_else(|| Error::NotFound(format!("sync session {id}")))?
            .into_sync()
    }
    /// Capture a local baseline and populate an empty target. Reusing the session
    /// ID reconnects the recorded baseline without silently publishing new edits.
    pub async fn initialize(
        &self,
        id: OperationId,
        local_root: impl AsRef<Path>,
        destination: &Workspace,
        quiescent: bool,
    ) -> Result<SyncSession> {
        let root = local_root.as_ref().to_path_buf();
        let guard = self.operations.clone().lock_owned().await;
        let lease = self.receipts.acquire(&id)?;
        let service = self.clone();
        let destination = destination.clone();
        owned(async move {
            let (_guard, _lease) = (guard, lease);
            service
                .initialize_inner(id, &root, &destination, quiescent)
                .await
        })
        .await
    }
    async fn initialize_inner(
        &self,
        id: OperationId,
        local_root: &Path,
        destination: &Workspace,
        quiescent: bool,
    ) -> Result<SyncSession> {
        require_quiescent(quiescent)?;
        let local_root = fs::canonicalize(local_root)?;
        if let Some(value) = self.receipts.load(&id)? {
            let mut session = value.into_sync()?;
            if session.local_root != local_root
                || !same_ref(&session.workspace, &destination.reference())?
            {
                return Err(Error::Conflict(
                    "sync ID belongs to another source or target".into(),
                ));
            }
            // Restore is idempotent and checks that the target still matches.
            if !session.initialized {
                self.restore_checkpoint_inner(&session.baseline, destination, quiescent)
                    .await?;
                session.initialized = true;
                self.receipts
                    .save(&session.id, &WorkflowRecord::Sync(session.clone()))?;
            }
            return Ok(session);
        }
        let engine = self.checkpoints.clone();
        let root = local_root.clone();
        let checkpoint = blocking(move || engine.capture(&root, CaptureOptions::default())).await?;
        let mut session = SyncSession {
            id,
            workspace: destination.reference(),
            local_root,
            baseline: checkpoint.reference().clone(),
            latest_reconciliation: None,
            pending_remote: None,
            initialized: false,
            last_apply: None,
        };
        // Persist before upload so retries retain the same baseline.
        self.receipts
            .save(&session.id, &WorkflowRecord::Sync(session.clone()))?;
        self.restore_checkpoint_inner(&session.baseline, destination, quiescent)
            .await?;
        session.initialized = true;
        self.receipts
            .save(&session.id, &WorkflowRecord::Sync(session.clone()))?;
        Ok(session)
    }
    /// Export the remote tree and stage a three-way merge for review. The local
    /// project is untouched; conflicts include all three preserved versions.
    pub async fn sync_back(
        &self,
        id: &OperationId,
        source: &Workspace,
        source_quiescent: bool,
    ) -> Result<Reconciliation> {
        let guard = self.operations.clone().lock_owned().await;
        let lease = self.receipts.acquire(id)?;
        let service = self.clone();
        let id = id.clone();
        let source = source.clone();
        owned(async move {
            let (_guard, _lease) = (guard, lease);
            service
                .sync_back_inner(&id, &source, source_quiescent)
                .await
        })
        .await
    }
    async fn sync_back_inner(
        &self,
        id: &OperationId,
        source: &Workspace,
        source_quiescent: bool,
    ) -> Result<Reconciliation> {
        let mut session = self.session(id)?;
        if !session.initialized {
            return Err(Error::Conflict(
                "initialize the target successfully before syncing back".into(),
            ));
        }
        if !same_ref(&session.workspace, &source.reference())? {
            return Err(Error::Conflict(
                "sync source differs from the session target".into(),
            ));
        }
        // A prior apply may have published files before its workflow receipt
        // reached disk. Recover that baseline before observing newer remote edits.
        if let Some(previous) = session.latest_reconciliation.clone() {
            let engine = self.checkpoints.clone();
            match blocking(move || engine.inspect_apply(&previous)).await? {
                ApplyState::Applied { receipt } => {
                    session.baseline = session.pending_remote.take().ok_or_else(|| {
                        Error::State("applied sync is missing its remote baseline".into())
                    })?;
                    session.last_apply = Some(receipt);
                    session.latest_reconciliation = None;
                    self.receipts
                        .save(id, &WorkflowRecord::Sync(session.clone()))?;
                }
                ApplyState::Interrupted { recovery } => {
                    return Err(Error::Conflict(format!(
                        "finish or roll back interrupted apply {} before syncing again",
                        recovery.0
                    )))
                }
                ApplyState::NotStarted | ApplyState::RolledBack { .. } => {}
            }
        }
        let remote = self
            .capture_workspace_inner(source, source_quiescent)
            .await?;
        let engine = self.checkpoints.clone();
        let base = session.baseline.clone();
        let root = session.local_root.clone();
        let reference = remote.reference().clone();
        let review = blocking(move || engine.reconcile(&base, &root, &reference)).await?;
        session.latest_reconciliation = Some(review.reference.clone());
        session.pending_remote = Some(remote.reference().clone());
        self.receipts
            .save(id, &WorkflowRecord::Sync(session.clone()))?;
        Ok(review)
    }
    /// The caller resolves conflicts in the review directory, acknowledges their
    /// paths through ICheckpointEngine::accept_resolution, and then applies here.
    pub async fn apply_sync(
        &self,
        id: &OperationId,
        review: &Reconciliation,
        options: ApplyOptions,
    ) -> Result<ApplyReceipt> {
        let _guard = self.operations.lock().await;
        let _lease = self.receipts.acquire(id)?;
        let mut session = self.session(id)?;
        if let Some(receipt) = &session.last_apply {
            if receipt.reconciliation == review.reference {
                let engine = self.checkpoints.clone();
                let reference = review.reference.clone();
                return blocking(move || {
                    let _held_lease = _lease;
                    engine.apply(&reference, options)
                })
                .await;
            }
        }
        if session.latest_reconciliation.as_ref() != Some(&review.reference) {
            return Err(Error::Conflict(
                "review is not the session's latest reconciliation".into(),
            ));
        }
        let next_baseline = session
            .pending_remote
            .clone()
            .ok_or_else(|| Error::State("sync receipt omitted pending remote checkpoint".into()))?;
        let engine = self.checkpoints.clone();
        let reference = review.reference.clone();
        let receipts = self.receipts.clone();
        let id = id.clone();
        blocking(move || {
            // A canceled async caller does not release the workflow lease while
            // the blocking filesystem publication is still running.
            let _held_lease = _lease;
            let applied = engine.apply(&reference, options)?;
            session.baseline = next_baseline;
            session.pending_remote = None;
            session.latest_reconciliation = None;
            session.last_apply = Some(applied.clone());
            receipts.save(&id, &WorkflowRecord::Sync(session.clone()))?;
            Ok(applied)
        })
        .await
    }
    /// Observer cancellation leaves the accepted transfer running through cleanup.
    pub async fn capture_workspace(
        &self,
        source: &Workspace,
        quiescent: bool,
    ) -> Result<Checkpoint> {
        let guard = self.operations.clone().lock_owned().await;
        let service = self.clone();
        let source = source.clone();
        owned(async move {
            let _guard = guard;
            service.capture_workspace_inner(&source, quiescent).await
        })
        .await
    }
    async fn capture_workspace_inner(
        &self,
        source: &Workspace,
        quiescent: bool,
    ) -> Result<Checkpoint> {
        require_quiescent(quiescent)?;
        let transfer = TreeTransfer::new();
        let outcome = async {
            let metadata = source.backend().export_tree(&transfer).await?;
            let expected = metadata.bytes;
            if expected as u128 > MAX_ARCHIVE as u128 {
                return Err(Error::InvalidInput(
                    "target snapshot exceeds transfer limit".into(),
                ));
            }
            let mut bytes = Vec::with_capacity(expected as usize);
            let mut offset = 0;
            loop {
                let chunk = source
                    .read_file(ReadFile {
                        path: transfer.relative_archive_path(),
                        offset,
                        limit: CHUNK as u32,
                    })
                    .await?;
                if chunk.bytes.len() > CHUNK
                    || offset.checked_add(chunk.bytes.len() as u64) != Some(chunk.next_offset)
                    || chunk.next_offset > expected
                    || (chunk.bytes.is_empty() && !chunk.eof)
                {
                    return Err(Error::State("invalid file-transfer cursor".into()));
                }
                bytes.extend_from_slice(&chunk.bytes);
                offset = chunk.next_offset;
                if chunk.eof {
                    break;
                }
            }
            let hash = format!("{:x}", Sha256::digest(&bytes));
            if offset != expected || metadata.sha256 != hash {
                return Err(Error::State("snapshot transfer checksum mismatch".into()));
            }
            let engine = self.checkpoints.clone();
            blocking(move || engine.capture_archive(&bytes, CaptureOptions::default())).await
        }
        .await;
        self.cleanup(source, &transfer).await;
        outcome
    }
    /// Populate an empty destination, or retry an identical restoration. This
    /// never overwrites a populated target with a different snapshot.
    /// Observer cancellation leaves the accepted restoration running through cleanup.
    pub async fn restore_checkpoint(
        &self,
        checkpoint: &CheckpointRef,
        destination: &Workspace,
        quiescent: bool,
    ) -> Result<()> {
        let guard = self.operations.clone().lock_owned().await;
        let service = self.clone();
        let checkpoint = checkpoint.clone();
        let destination = destination.clone();
        owned(async move {
            let _guard = guard;
            service
                .restore_checkpoint_inner(&checkpoint, &destination, quiescent)
                .await
        })
        .await
    }
    async fn restore_checkpoint_inner(
        &self,
        checkpoint: &CheckpointRef,
        destination: &Workspace,
        quiescent: bool,
    ) -> Result<()> {
        require_quiescent(quiescent)?;
        let engine = self.checkpoints.clone();
        let reference = checkpoint.clone();
        let bytes = blocking(move || engine.export_checkpoint_archive(&reference)).await?;
        if bytes.len() > MAX_ARCHIVE {
            return Err(Error::InvalidInput(
                "checkpoint exceeds transfer limit".into(),
            ));
        }
        let transfer = TreeTransfer::new();
        let outcome = async {
            for (index, chunk) in bytes.chunks(CHUNK).enumerate() {
                destination
                    .write_file(WriteFile {
                        path: transfer.relative_archive_path(),
                        offset: (index * CHUNK) as u64,
                        bytes: chunk.to_vec(),
                        truncate: index == 0,
                        executable: false,
                    })
                    .await?;
            }
            destination.backend().import_tree(&transfer).await?;
            Ok(())
        }
        .await;
        self.cleanup(destination, &transfer).await;
        outcome
    }
    /// Handoff a stopped run's portable files to an empty target and launch a
    /// caller-supplied continuation command. Checkpoint-capable agent state must
    /// already be written into allowed project files. No VM memory is migrated.
    /// The source must already be terminal; this service never guesses whether
    /// a Lost/unknown process is safe to duplicate. Failure leaves it stopped.
    pub async fn handoff(
        &self,
        id: OperationId,
        source: &Workspace,
        source_run: &Run,
        destination: &Workspace,
        continuation: ExecutionRequest,
        quiescent: bool,
    ) -> Result<HandoffReceipt> {
        let guard = self.operations.clone().lock_owned().await;
        let lease = self.receipts.acquire(&id)?;
        let service = self.clone();
        let source = source.clone();
        let source_run = source_run.clone();
        let destination = destination.clone();
        owned(async move {
            let (_guard, _lease) = (guard, lease);
            service
                .handoff_inner(
                    id,
                    &source,
                    &source_run,
                    &destination,
                    continuation,
                    quiescent,
                )
                .await
        })
        .await
    }
    async fn handoff_inner(
        &self,
        id: OperationId,
        source: &Workspace,
        source_run: &Run,
        destination: &Workspace,
        continuation: ExecutionRequest,
        quiescent: bool,
    ) -> Result<HandoffReceipt> {
        require_quiescent(quiescent)?;
        if same_ref(&source.reference(), &destination.reference())? {
            return Err(Error::InvalidInput(
                "handoff requires a different destination".into(),
            ));
        }
        if !same_ref(&source_run.reference().workspace, &source.reference())? {
            return Err(Error::InvalidInput(
                "source run belongs to another workspace".into(),
            ));
        }
        continuation.command.validate()?;
        let identity = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(
                source_run.reference(),
                destination.reference(),
                &continuation.command,
                &continuation.secrets.0,
                continuation.private_stdin.as_ref().map(SecretInput::expose)
            ))?)
        );
        let mut receipt = if let Some(value) = self.receipts.load(&id)? {
            let receipt = value.into_handoff()?;
            if receipt.identity != identity {
                return Err(Error::Conflict(
                    "handoff ID was reused for different inputs".into(),
                ));
            }
            receipt
        } else {
            ensure_source_stopped(source_run).await?;
            let checkpoint = self.capture_workspace_inner(source, quiescent).await?;
            let receipt = HandoffReceipt {
                id: id.clone(),
                identity,
                source: source_run.reference(),
                destination: destination.reference(),
                checkpoint: checkpoint.reference().clone(),
                phase: HandoffPhase::Captured,
                destination_run: None,
            };
            self.receipts
                .save(&id, &WorkflowRecord::Handoff(receipt.clone()))?;
            receipt
        };
        if matches!(receipt.phase, HandoffPhase::Started) {
            return Ok(receipt);
        }
        ensure_source_stopped(source_run).await?;
        if matches!(receipt.phase, HandoffPhase::Captured) {
            self.restore_checkpoint_inner(&receipt.checkpoint, destination, quiescent)
                .await?;
            receipt.phase = HandoffPhase::Restored;
            self.receipts
                .save(&id, &WorkflowRecord::Handoff(receipt.clone()))?;
        }
        ensure_source_stopped(source_run).await?;
        let run_id = RunId::new(format!(
            "handoff-{:x}",
            Sha256::digest(id.as_str().as_bytes())
        ))?;
        let run = destination
            .start_with(LaunchRequest {
                id: run_id,
                execution: continuation,
                output_limit_bytes: 8 * 1024 * 1024,
            })
            .await?;
        receipt.destination_run = Some(run.reference());
        receipt.phase = HandoffPhase::Started;
        self.receipts
            .save(&id, &WorkflowRecord::Handoff(receipt.clone()))?;
        Ok(receipt)
    }
    async fn cleanup(&self, workspace: &Workspace, transfer: &TreeTransfer) {
        // The backend derives the only removable directory from the validated ID.
        let _ = workspace.backend().remove_transfer(transfer).await;
    }
}

fn require_quiescent(value: bool) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err(Error::Conflict(
            "stop all writers and assert quiescent before copying a workspace".into(),
        ))
    }
}
async fn ensure_source_stopped(run: &Run) -> Result<()> {
    if run.state().await?.is_terminal() {
        Ok(())
    } else {
        Err(Error::Conflict(
            "handoff source must have a confirmed terminal run state".into(),
        ))
    }
}
fn same_ref(a: &WorkspaceRef, b: &WorkspaceRef) -> Result<bool> {
    Ok(a == b)
}
async fn owned<T: Send + 'static>(
    operation: impl Future<Output = Result<T>> + Send + 'static,
) -> Result<T> {
    tokio::spawn(operation)
        .await
        .map_err(|error| Error::State(format!("sync worker failed: {error}")))?
}
async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    spawn_blocking(operation)
        .await
        .map_err(|e| Error::State(format!("checkpoint task failed: {e}")))?
}

#[cfg(test)]
#[path = "sync_service_tests.rs"]
mod tests;
