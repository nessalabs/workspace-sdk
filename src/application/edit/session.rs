use super::candidate::Candidate;
use crate::{
    checkpoint::{
        ApplyOptions, ApplyReceipt, ApplyState, CaptureOptions, Checkpoint, ICheckpointEngine,
        Reconciliation, ReconciliationRef,
    },
    ports::IEditCoordinator,
    EditChange, EditRevision, Error, FileChunk, Result, WorkspacePath,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

/// Blocking selected-tree edit session for one local directory. Use a blocking
/// executor in async hosts and keep ownership/leases inside its closure through
/// completion; canceling an async waiter is not cancellation of filesystem work.
///
/// Candidate mutations never touch the live project. All destination observations
/// and publication require an explicit caller assertion that unmanaged writers
/// are stopped. The coordinator only serializes participants sharing its root
/// and lease repository. No hash-then-write CAS or editor fencing is implied.
///
/// Sessions are not cloneable or restartable. Tokens reject stale/cross-session
/// requests. At most 4096 entries, 8 MiB selected bytes and 32 snapshot-producing
/// staging attempts, 16 reviews and 16 resolutions are supported. These budgets
/// are separate so the last accepted stage can still be reviewed and resolved.
/// Immutable checkpoints/reviews/recovery remain in the
/// engine after drop, for application-owned retention and explicit inspection.
pub struct ManagedEditSession {
    coordinator: Arc<dyn IEditCoordinator>,
    engine: Arc<dyn ICheckpointEngine>,
    baseline: Checkpoint,
    candidate: Candidate,
    nonce: String,
    generation: u32,
    stage_attempts: u32,
    review_attempts: u32,
    resolution_attempts: u32,
    latest: Option<Reconciliation>,
}
impl ManagedEditSession {
    /// Capture a baseline under the cooperative lease and asserted external
    /// quiescence. No project writes occur. Secret/ignore selection is engine-owned.
    pub fn begin(
        coordinator: Arc<dyn IEditCoordinator>,
        engine: Arc<dyn ICheckpointEngine>,
        quiescent: bool,
    ) -> Result<Self> {
        require_quiescent(quiescent)?;
        let actual = engine.limits();
        let required = Self::checkpoint_limits();
        if actual.max_files > required.max_files
            || actual.max_file_bytes > required.max_file_bytes
            || actual.max_total_bytes > required.max_total_bytes
            || actual.max_bundle_bytes > required.max_bundle_bytes
            || actual.max_manifest_bytes > required.max_manifest_bytes
        {
            return Err(Error::Unsupported("configure checkpoint engine with ManagedEditSession::checkpoint_limits or stricter bounds".into()));
        }
        let _lease = coordinator.acquire()?;
        let baseline = engine.capture(coordinator.root(), CaptureOptions::default())?;
        let candidate = Candidate::load(engine.as_ref(), baseline.clone())?;
        Ok(Self {
            coordinator,
            engine,
            baseline,
            candidate,
            nonce: Uuid::new_v4().to_string(),
            generation: 0,
            stage_attempts: 0,
            review_attempts: 0,
            resolution_attempts: 0,
            latest: None,
        })
    }
    /// Configure the injected engine with these bounds or stricter limits before
    /// beginning a session. They govern baseline, live captures, merged results,
    /// resolution and recovery, not just the in-memory candidate.
    pub fn checkpoint_limits() -> crate::checkpoint::CheckpointLimits {
        crate::checkpoint::CheckpointLimits {
            max_files: 4096,
            max_file_bytes: 8 * 1024 * 1024,
            max_total_bytes: 8 * 1024 * 1024,
            max_bundle_bytes: 32 * 1024 * 1024,
            max_manifest_bytes: 16 * 1024 * 1024,
        }
    }
    /// Current immutable candidate metadata. This does not describe today's live files.
    pub fn checkpoint(&self) -> &Checkpoint {
        &self.candidate.checkpoint
    }
    /// Revision of the complete candidate, qualified to this session and generation.
    pub fn revision(&self) -> EditRevision {
        EditRevision(format!(
            "{:x}",
            Sha256::digest(format!(
                "{}:{}:{}",
                self.nonce, self.generation, self.candidate.checkpoint.manifest.tree_fingerprint
            ))
        ))
    }
    /// Read at most 256 KiB from a candidate regular file under its exact revision.
    /// Binary bytes are decoded in memory within the session's 8 MiB tree bound.
    pub fn read(
        &self,
        expected: &EditRevision,
        path: &WorkspacePath,
        offset: u64,
        limit: u32,
    ) -> Result<FileChunk> {
        self.check_revision(expected)?;
        if !(1..=262144).contains(&limit) {
            return Err(Error::InvalidInput(
                "candidate read limit must be 1..=262144".into(),
            ));
        }
        let entry = self
            .candidate
            .archive
            .entries
            .iter()
            .find(|entry| entry.file.path == path.as_str())
            .ok_or_else(|| Error::NotFound("candidate read path".into()))?;
        if entry.file.kind != crate::checkpoint::EntryKind::File {
            return Err(Error::Unsupported(
                "candidate reads do not follow symlinks".into(),
            ));
        }
        if offset > entry.file.size {
            return Err(Error::InvalidInput(
                "candidate read offset exceeds file".into(),
            ));
        }
        let bytes = STANDARD
            .decode(&entry.data_base64)
            .map_err(|_| Error::State("invalid candidate content encoding".into()))?;
        let end = offset
            .saturating_add(u64::from(limit))
            .min(bytes.len() as u64) as usize;
        Ok(FileChunk {
            bytes: bytes[offset as usize..end].to_vec(),
            next_offset: end as u64,
            eof: end == bytes.len(),
        })
    }
    /// Stage one revision-checked candidate mutation. Destination remains untouched.
    /// Success invalidates the old revision and any unapplied review. Failed requests
    /// retain candidate/review; a snapshot-producing attempt still consumes budget.
    pub fn stage(&mut self, expected: &EditRevision, change: EditChange) -> Result<EditRevision> {
        self.check_revision(expected)?;
        let _lease = self.coordinator.acquire()?;
        self.ensure_editable()?;
        let (archive, bytes) = self.candidate.prepare(change)?;
        if self.review_attempts >= 16 || self.resolution_attempts >= 16 {
            return Err(Error::Unsupported(
                "session completion budget exhausted; candidate cannot be advanced".into(),
            ));
        }
        reserve_attempt(&mut self.stage_attempts, 32, "staging")?;
        let candidate = Candidate::capture(
            self.engine.as_ref(),
            &self.candidate.checkpoint,
            archive,
            bytes,
        )?;
        self.candidate = candidate;
        self.generation += 1;
        self.latest = None;
        Ok(self.revision())
    }
    /// Reconcile baseline/current live tree/candidate under asserted quiescence.
    /// GitCheckpointEngine owns all three-way merging and preserved conflict sides.
    /// A review never writes the destination. A later successful stage invalidates it.
    pub fn review(&mut self, expected: &EditRevision, quiescent: bool) -> Result<Reconciliation> {
        self.check_revision(expected)?;
        require_quiescent(quiescent)?;
        let _lease = self.coordinator.acquire()?;
        self.ensure_editable()?;
        if self.resolution_attempts >= 16 {
            return Err(Error::Unsupported(
                "session resolution budget exhausted; cannot open another review".into(),
            ));
        }
        reserve_attempt(&mut self.review_attempts, 16, "review")?;
        let review = self.engine.reconcile(
            self.baseline.reference(),
            self.coordinator.root(),
            self.candidate.checkpoint.reference(),
        )?;
        self.latest = Some(review.clone());
        Ok(review)
    }
    /// Accept manually edited private review files, acknowledging each conflict.
    /// Resolution uses the engine's existing validation. Live writers need not stay
    /// paused during private review; apply separately requires a fresh boundary.
    pub fn accept_resolution(
        &mut self,
        expected: &EditRevision,
        review: &ReconciliationRef,
        resolved_paths: &[String],
    ) -> Result<Reconciliation> {
        self.check_revision(expected)?;
        self.check_review(review)?;
        if resolved_paths.len() > 4096 || resolved_paths.iter().any(|path| path.len() > 4096) {
            return Err(Error::InvalidInput(
                "resolution acknowledgement exceeds session bounds".into(),
            ));
        }
        let _lease = self.coordinator.acquire()?;
        self.ensure_editable()?;
        reserve_attempt(&mut self.resolution_attempts, 16, "resolution")?;
        let accepted = self.engine.accept_resolution(review, resolved_paths)?;
        self.latest = Some(accepted.clone());
        Ok(accepted)
    }
    /// Publish only the latest session review, while holding its cooperative lease
    /// and requiring caller-established quiescence. The stored engine fingerprint
    /// rejects changed destinations. It does not fence arbitrary editors.
    /// Completed retry is engine-idempotent and preserves later edits. Interruption
    /// remains inspectable and blocks replacing the pending review/candidate.
    pub fn apply(
        &self,
        expected: &EditRevision,
        review: &ReconciliationRef,
        quiescent: bool,
    ) -> Result<ApplyReceipt> {
        self.check_revision(expected)?;
        require_quiescent(quiescent)?;
        let staged = self.check_review(review)?;
        let _lease = self.coordinator.acquire()?;
        self.engine.apply(
            review,
            ApplyOptions {
                quiescent: true,
                expected_fingerprint: staged.expected_fingerprint.clone(),
            },
        )
    }
    /// Inspect durable engine progress for the latest review without changing files.
    /// Persist the review reference externally if recovery inspection must survive
    /// session loss; use ICheckpointEngine after reopening its same private store.
    pub fn inspect_apply(&self, review: &ReconciliationRef) -> Result<ApplyState> {
        self.check_review(review)?;
        self.engine.inspect_apply(review)
    }
    fn check_revision(&self, expected: &EditRevision) -> Result<()> {
        if *expected != self.revision() {
            return Err(Error::Conflict(
                "stale or foreign edit-session revision".into(),
            ));
        }
        Ok(())
    }
    fn check_review(&self, review: &ReconciliationRef) -> Result<&Reconciliation> {
        self.latest
            .as_ref()
            .filter(|latest| latest.reference == *review)
            .ok_or_else(|| {
                Error::Conflict("review is not this session's latest candidate review".into())
            })
    }
    fn ensure_editable(&self) -> Result<()> {
        if let Some(review) = &self.latest {
            match self.engine.inspect_apply(&review.reference)? {
                ApplyState::Applied { .. } => {
                    return Err(Error::Conflict(
                        "session already applied; begin a new session".into(),
                    ))
                }
                ApplyState::Interrupted { .. } => {
                    return Err(Error::Conflict(
                        "pending apply is interrupted; inspect/recover before replacing it".into(),
                    ))
                }
                ApplyState::NotStarted | ApplyState::RolledBack { .. } => {}
            }
        }
        Ok(())
    }
}
fn require_quiescent(quiescent: bool) -> Result<()> {
    if !quiescent {
        return Err(Error::Conflict(
            "stop all unmanaged writers and assert quiescence for this entire operation".into(),
        ));
    }
    Ok(())
}

fn reserve_attempt(attempts: &mut u32, limit: u32, action: &str) -> Result<()> {
    if *attempts >= limit {
        return Err(Error::Unsupported(format!(
            "edit session exhausted its {limit} {action} snapshot attempts"
        )));
    }
    *attempts += 1;
    Ok(())
}
