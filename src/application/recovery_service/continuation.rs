//! Cooperative successors retain only the exact acknowledged current workspace.
use super::{digest, owned, RecoveryService};
use crate::{domain::*, Error, Result};

/// One cooperative successor in the current acknowledged workspace.
///
/// Keep all identity/head fields and the command unchanged across retries.
/// This request cannot select an arbitrary target or overwrite divergent files.
pub struct RecoveryContinuationRequest {
    /// Stable successor identity within this session and expected previous epoch.
    pub request_id: OperationId,
    /// Exact preceding epoch whose acknowledged workspace should be retained.
    pub expected_previous_epoch: u64,
    /// Exact complete canonical head observed before requesting the successor.
    pub expected_committed: RecoveryHead,
    /// Transient execution inputs, with the same credential rules as recovery.
    pub continuation: ExecutionRequest,
    /// Caller guarantees exclusive access before new/prelaunch advancement and stops
    /// ALL other writers, including direct runs, editors and unmanaged processes.
    /// Must be true on retries too, repeating the original admission assertion.
    /// After launch intent it does not assert that the accepted run is now idle.
    /// Native predecessor inspection and selected
    /// fingerprints do not establish a global filesystem or process fence.
    pub quiescent: bool,
}

impl RecoveryService {
    /// Continue a successful, acknowledged cooperative step at the same root.
    ///
    /// Under the existing recovery lease, require the preceding authoritative
    /// run to have exited zero and the selected tree to equal the canonical head.
    /// Pin a new epoch/run before launch, retaining the exact WorkspaceRef.
    /// No restore, deletion, provider selection or external-effects fence occurs.
    ///
    /// Divergent selected data is preserved and refused. The caller must exclude
    /// all other writers; excluded paths are outside checkpoint comparison.
    /// After durable launch intent, retry never recaptures or restores an older
    /// baseline. An accepted retry returns the same run even after publication.
    /// Unknown outcomes retain their identities and block a new attempt.
    /// Cancellation, repository durability and provider bounds are the same as
    /// [`Self::recover`]; dropping the caller does not cancel the owned use case.
    /// Keep caller-established exclusive access until that owned operation
    /// finishes, even if its waiter is dropped. Cancellation is not a fence
    /// against external writers.
    pub async fn continue_current(
        &self,
        id: &OperationId,
        request: RecoveryContinuationRequest,
    ) -> Result<RecoverySession> {
        let service = self.clone();
        let id = id.clone();
        owned(tokio::spawn(async move {
            service.continue_current_owned(&id, request).await
        }))
        .await
    }

    async fn continue_current_owned(
        &self,
        id: &OperationId,
        request: RecoveryContinuationRequest,
    ) -> Result<RecoverySession> {
        request.continuation.command.validate()?;
        if !request.quiescent {
            return Err(Error::Conflict(
                "hold exclusive access and stop all other writers".into(),
            ));
        }
        let _lease = self.repository.acquire_recovery(id)?;
        let mut session = self.session(id)?;
        let attempt = session.current_attempt.as_ref().ok_or_else(|| {
            Error::Conflict("continuation requires an acknowledged predecessor".into())
        })?;
        let retry = attempt.request_id == request.request_id
            && attempt.expected_previous_epoch == request.expected_previous_epoch;
        if retry {
            if attempt.retained.is_none() || attempt.checkpoint != request.expected_committed {
                return Err(Error::Conflict(
                    "continuation kind or pinned head changed".into(),
                ));
            }
        } else {
            if attempt.epoch != request.expected_previous_epoch
                || session.committed != request.expected_committed
                || attempt.checkpoint.generation >= session.committed.generation
                || attempt.request_id == request.request_id
            {
                return Err(Error::Conflict(
                    "continuation requires the current acknowledged epoch and head".into(),
                ));
            }
            let RecoveryPhase::Started { run, .. } = &attempt.phase else {
                return Err(Error::Conflict(
                    "finish the pinned uncertain attempt before continuing".into(),
                ));
            };
            let retained = RecoveryRetainedWorkspace {
                predecessor: run.clone(),
                workspace_epoch: attempt
                    .retained
                    .as_ref()
                    .map_or(attempt.epoch, |target| target.workspace_epoch),
                launch_identity: digest(&request.continuation.command)?,
            };
            self.verify_retained(&retained, &session.committed).await?;
            let epoch = session.next_epoch()?;
            let mut next = session.clone();
            next.current_attempt = Some(RecoveryAttempt {
                request_id: request.request_id,
                expected_previous_epoch: attempt.epoch,
                epoch,
                provider: attempt.provider.clone(),
                workspace_id: retained.predecessor.workspace.id.clone(),
                retained: Some(retained),
                run_id: RecoverySession::attempt_ids(id, epoch)?.1,
                checkpoint: session.committed.clone(),
                phase: RecoveryPhase::Preparing,
            });
            self.replace(&mut session, next)?;
        }
        // Reuse only this call's verified boundary while its same lease and
        // caller-established exclusivity are held. A prelaunch retry recaptures.
        self.resume(session, request.continuation, !retry).await
    }

    pub(super) async fn verify_retained(
        &self,
        retained: &RecoveryRetainedWorkspace,
        head: &RecoveryHead,
    ) -> Result<()> {
        let workspace = self.sdk.connect(&retained.predecessor.workspace).await?;
        if workspace
            .run(retained.predecessor.id.clone())
            .state()
            .await?
            != (RunState::Exited { exit_code: 0 })
        {
            return Err(Error::Conflict(
                "continuation predecessor is not confirmed successfully stopped".into(),
            ));
        }
        let engine = self.sync.checkpoints();
        let reference = head.checkpoint.clone();
        let canonical_fingerprint = tokio::task::spawn_blocking(move || {
            engine
                .export(&reference)
                .map(|bundle| bundle.manifest.tree_fingerprint)
        })
        .await
        .map_err(|error| Error::State(format!("checkpoint verification failed: {error}")))??;
        let current = self.sync.capture_workspace(&workspace, true).await?;
        if current.manifest.tree_fingerprint != canonical_fingerprint {
            return Err(Error::Conflict(
                "current selected files differ from the acknowledged checkpoint".into(),
            ));
        }
        Ok(())
    }
}
