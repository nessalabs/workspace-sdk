//! Explicit checkpoint-boundary placement with durable, retryable launch intent.
mod continuation;

pub use continuation::RecoveryContinuationRequest;

use super::{SyncService, WorkspaceSdk};
use crate::{checkpoint::CheckpointRef, domain::*, ports::IRecoveryRepository, Error, Result};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Evidence required before replacing a previously accepted run.
#[derive(Clone, Copy, Debug)]
pub enum RecoverySafety {
    /// Query the previous native run and require a confirmed terminal state.
    ConfirmedStopped,
    /// The caller guarantees the old process cannot perform unfenced external
    /// effects. This assertion is not verified by the SDK. Isolated workspaces
    /// and epoch CAS fence canonical publication, not arbitrary API calls.
    ExternallyFenced,
}

/// One explicit placement request. Keep its ID and expected epoch across retries.
pub struct RecoveryRequest {
    /// Stable identity within this session and expected previous epoch.
    pub request_id: OperationId,
    /// Last observed epoch, zero before the first placement.
    pub expected_previous_epoch: u64,
    /// Caller-observed available locations; selection follows session priority.
    /// This is a placement hint, not proof that an unavailable process stopped.
    pub available: Vec<ProviderId>,
    /// Transient launch inputs. Authority fingerprints only the nonsecret command;
    /// the native salted claim binds credentials when launch is attempted.
    pub continuation: ExecutionRequest,
    /// Required evidence before replacing an accepted previous run.
    pub safety: RecoverySafety,
}

/// Coordinates explicit recovery using existing providers, native launch claims
/// and checkpoint transfers. It does not watch health, replicate storage offsite,
/// or migrate process memory. Authority and artifacts must survive the failed host.
///
/// Fresh placements get isolated workspaces; explicit acknowledged successors
/// can retain the current workspace. Old directories are retained.
/// A nonexpiring repository lease serializes effects for one session; short CAS
/// writes also reject stale publications. A controller cannot steal a live lease.
/// Once polled, a use case runs in an owned Tokio task: dropping its caller does
/// not cancel remote effects, cleanup, or release its lease. Reconcile via
/// `session` and the same request. Runtime/process loss can interrupt that task;
/// pinned identity still governs retry. Provider implementations must bound their
/// own calls. This service does not impose a timeout that could hide remote work.
#[derive(Clone)]
pub struct RecoveryService {
    sdk: Arc<WorkspaceSdk>,
    sync: Arc<SyncService>,
    repository: Arc<dyn IRecoveryRepository>,
}

impl RecoveryService {
    /// Reuse the configured SDK and existing concrete checkpoint transfer service.
    pub fn new(
        sdk: Arc<WorkspaceSdk>,
        sync: Arc<SyncService>,
        repository: Arc<dyn IRecoveryRepository>,
    ) -> Self {
        Self {
            sdk,
            sync,
            repository,
        }
    }

    /// Load the current validated authority snapshot without polling a provider.
    pub fn session(&self, id: &OperationId) -> Result<RecoverySession> {
        self.repository
            .load_recovery(id)?
            .ok_or_else(|| Error::NotFound(format!("recovery session {id}")))
    }

    /// Reserve an immutable placement configuration and complete seed checkpoint.
    /// Retrying never replaces a later published head with the original seed.
    /// Candidate options must create fresh targets; attaching populated projects
    /// is refused by the existing empty-target import boundary.
    pub async fn create(
        &self,
        id: OperationId,
        candidates: Vec<RecoveryCandidate>,
        seed: CheckpointRef,
    ) -> Result<RecoverySession> {
        let service = self.clone();
        owned(tokio::spawn(async move {
            service.create_owned(id, candidates, seed).await
        }))
        .await
    }

    async fn create_owned(
        &self,
        id: OperationId,
        candidates: Vec<RecoveryCandidate>,
        seed: CheckpointRef,
    ) -> Result<RecoverySession> {
        let _lease = self.repository.acquire_recovery(&id)?;
        let identity = digest(&(&candidates, &seed))?;
        if let Some(existing) = self.repository.load_recovery(&id)? {
            if existing.identity != identity {
                return Err(Error::Conflict(
                    "recovery session configuration changed".into(),
                ));
            }
            return Ok(existing);
        }
        let session = RecoverySession {
            id,
            identity,
            candidates,
            revision: 0,
            committed: RecoveryHead {
                generation: 0,
                checkpoint: seed.clone(),
            },
            current_attempt: None,
        };
        session.validate_replacement(None)?;
        let engine = self.sync.checkpoints();
        // Validate availability before publishing a recovery point. This does
        // not make an application-owned artifact store remotely durable.
        tokio::task::spawn_blocking(move || engine.export(&seed))
            .await
            .map_err(|e| Error::State(format!("checkpoint verification failed: {e}")))??;
        self.repository
            .compare_exchange_recovery(&session.id, None, &session)?;
        Ok(session)
    }

    /// Place or resume one pinned attempt. Only a new explicit request can move
    /// to another location; availability changes on retry do not retarget it.
    /// Provider/launch uncertainty keeps the same IDs and blocks a new epoch.
    pub async fn recover(
        &self,
        id: &OperationId,
        request: RecoveryRequest,
    ) -> Result<RecoverySession> {
        let service = self.clone();
        let id = id.clone();
        owned(tokio::spawn(async move {
            service.recover_owned(&id, request).await
        }))
        .await
    }

    async fn recover_owned(
        &self,
        id: &OperationId,
        request: RecoveryRequest,
    ) -> Result<RecoverySession> {
        request.continuation.command.validate()?;
        let _lease = self.repository.acquire_recovery(id)?;
        let mut session = self.session(id)?;
        let retry = session.current_attempt.as_ref().is_some_and(|attempt| {
            attempt.request_id == request.request_id
                && attempt.expected_previous_epoch == request.expected_previous_epoch
        });
        if retry
            && session
                .current_attempt
                .as_ref()
                .is_some_and(|attempt| attempt.retained.is_some())
        {
            return Err(Error::Conflict(
                "retry retained work through continue_current".into(),
            ));
        }
        if !retry {
            self.prepare(&mut session, &request).await?;
        }
        self.resume(session, request.continuation, false).await
    }

    async fn prepare(
        &self,
        session: &mut RecoverySession,
        request: &RecoveryRequest,
    ) -> Result<()> {
        let previous_epoch = session
            .current_attempt
            .as_ref()
            .map_or(0, |attempt| attempt.epoch);
        if request.expected_previous_epoch != previous_epoch {
            return Err(Error::Conflict("stale recovery request epoch".into()));
        }
        if let Some(attempt) = &session.current_attempt {
            let RecoveryPhase::Started { run, .. } = &attempt.phase else {
                return Err(Error::Conflict(
                    "finish the pinned uncertain attempt before moving".into(),
                ));
            };
            if matches!(request.safety, RecoverySafety::ConfirmedStopped) {
                let workspace = self.sdk.connect(&run.workspace).await?;
                if !workspace.run(run.id.clone()).state().await?.is_terminal() {
                    return Err(Error::Conflict(
                        "previous run is not confirmed stopped".into(),
                    ));
                }
            }
        }
        let candidate = session
            .candidates
            .iter()
            .find(|candidate| request.available.contains(&candidate.provider))
            .ok_or_else(|| Error::NotFound("no recovery location is available".into()))?;
        let epoch = session.next_epoch()?;
        let (workspace_id, run_id) = RecoverySession::attempt_ids(&session.id, epoch)?;
        let mut next = session.clone();
        next.current_attempt = Some(RecoveryAttempt {
            request_id: request.request_id.clone(),
            expected_previous_epoch: previous_epoch,
            epoch,
            provider: candidate.provider.clone(),
            workspace_id,
            retained: None,
            run_id,
            checkpoint: session.committed.clone(),
            phase: RecoveryPhase::Preparing,
        });
        self.replace(session, next)
    }

    async fn resume(
        &self,
        mut session: RecoverySession,
        execution: ExecutionRequest,
        retained_verified: bool,
    ) -> Result<RecoverySession> {
        // Do not persist an unsalted oracle for transient credential values.
        // The native target claim binds the complete execution with its own
        // private salt when a launch is attempted. Started retries return the
        // accepted reference without inspecting new transient credentials.
        let identity = digest(&execution.command)?;
        loop {
            let attempt = session
                .current_attempt
                .clone()
                .ok_or_else(|| Error::State("missing recovery attempt".into()))?;
            if let Some(retained) = &attempt.retained {
                require_launch_identity(&identity, &retained.launch_identity)?;
            }
            match attempt.phase {
                RecoveryPhase::Preparing => {
                    if let Some(retained) = &attempt.retained {
                        self.advance(
                            &mut session,
                            RecoveryPhase::Restored {
                                workspace: retained.predecessor.workspace.clone(),
                            },
                        )?;
                        continue;
                    }
                    let candidate = session
                        .candidates
                        .iter()
                        .find(|candidate| candidate.provider == attempt.provider)
                        .ok_or_else(|| Error::State("missing pinned recovery location".into()))?;
                    let workspace = self
                        .sdk
                        .open(
                            &attempt.provider,
                            OpenRequest {
                                workspace_id: attempt.workspace_id,
                                config: candidate.config.clone(),
                                resources: candidate.resources.clone(),
                            },
                        )
                        .await?;
                    if workspace.descriptor().ownership != Ownership::SdkOwned {
                        return Err(Error::Conflict(
                            "recovery requires a fresh SDK-owned workspace, not an attachment"
                                .into(),
                        ));
                    }
                    self.sync
                        .restore_checkpoint(&attempt.checkpoint.checkpoint, &workspace, true)
                        .await?;
                    self.advance(
                        &mut session,
                        RecoveryPhase::Restored {
                            workspace: workspace.reference(),
                        },
                    )?;
                }
                RecoveryPhase::Restored { workspace } => {
                    if let Some(retained) = &attempt.retained {
                        if !retained_verified {
                            self.verify_retained(retained, &attempt.checkpoint).await?;
                        }
                    }
                    self.advance(
                        &mut session,
                        RecoveryPhase::LaunchRequested {
                            workspace,
                            launch_identity: identity.clone(),
                        },
                    )?;
                }
                RecoveryPhase::LaunchRequested {
                    workspace,
                    launch_identity,
                } => {
                    require_launch_identity(&identity, &launch_identity)?;
                    let workspace = self.sdk.connect(&workspace).await?;
                    // Intent is already durable. Retrying this native claim must
                    // never restore the older checkpoint over process output.
                    let run = workspace
                        .start_with(LaunchRequest {
                            id: attempt.run_id,
                            execution: execution.clone(),
                            output_limit_bytes: 8 * 1024 * 1024,
                        })
                        .await?;
                    self.advance(
                        &mut session,
                        RecoveryPhase::Started {
                            run: run.reference(),
                            launch_identity,
                        },
                    )?;
                }
                RecoveryPhase::Started {
                    launch_identity, ..
                } => {
                    require_launch_identity(&identity, &launch_identity)?;
                    return Ok(session);
                }
            }
        }
    }

    /// Capture a terminal run's caller-established quiescent project and publish
    /// it only for the current epoch. Partial/failed capture never advances head.
    /// All unmanaged writers must also be stopped by the caller.
    /// This is a new capture on every call, not an idempotent publication request.
    /// After an ambiguous write error, inspect the session's committed head;
    /// calling again may publish another complete generation and retain artifacts.
    pub async fn publish(
        &self,
        id: &OperationId,
        epoch: u64,
        quiescent: bool,
    ) -> Result<RecoverySession> {
        let service = self.clone();
        let id = id.clone();
        owned(tokio::spawn(async move {
            service.publish_owned(&id, epoch, quiescent).await
        }))
        .await
    }

    async fn publish_owned(
        &self,
        id: &OperationId,
        epoch: u64,
        quiescent: bool,
    ) -> Result<RecoverySession> {
        let _lease = self.repository.acquire_recovery(id)?;
        let mut session = self.session(id)?;
        let attempt = session
            .current_attempt
            .as_ref()
            .filter(|attempt| attempt.epoch == epoch)
            .ok_or_else(|| Error::Conflict("stale recovery publication epoch".into()))?;
        let RecoveryPhase::Started { run, .. } = &attempt.phase else {
            return Err(Error::Conflict(
                "publish requires an accepted native run".into(),
            ));
        };
        if !quiescent {
            return Err(Error::Conflict("stop all writers before publishing".into()));
        }
        let workspace = self.sdk.connect(&run.workspace).await?;
        if !workspace.run(run.id.clone()).state().await?.is_terminal() {
            return Err(Error::Conflict(
                "publication run is not confirmed stopped".into(),
            ));
        }
        let checkpoint = self.sync.capture_workspace(&workspace, true).await?;
        let mut next = session.clone();
        next.committed = RecoveryHead {
            generation: session
                .committed
                .generation
                .checked_add(1)
                .ok_or_else(|| Error::Conflict("recovery generation exhausted".into()))?,
            checkpoint: checkpoint.reference().clone(),
        };
        self.replace(&mut session, next)?;
        Ok(session)
    }

    fn advance(&self, session: &mut RecoverySession, phase: RecoveryPhase) -> Result<()> {
        let mut next = session.clone();
        next.current_attempt
            .as_mut()
            .ok_or_else(|| Error::State("missing recovery attempt".into()))?
            .phase = phase;
        self.replace(session, next)
    }

    fn replace(&self, previous: &mut RecoverySession, mut next: RecoverySession) -> Result<()> {
        next.revision = previous.next_revision()?;
        self.repository
            .compare_exchange_recovery(&previous.id, Some(previous.revision), &next)?;
        *previous = next;
        Ok(())
    }
}

fn digest(value: &impl serde::Serialize) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}
async fn owned(task: tokio::task::JoinHandle<Result<RecoverySession>>) -> Result<RecoverySession> {
    task.await
        .map_err(|error| Error::State(format!("recovery task failed: {error}")))?
}
fn require_launch_identity(expected: &str, recorded: &str) -> Result<()> {
    if expected != recorded {
        return Err(Error::Conflict(
            "recovery continuation changed after launch intent".into(),
        ));
    }
    Ok(())
}
