//! One canonical recovery authority snapshot; process effects remain target-owned.
//!
//! Epoch checks fence publication to this record. They do not stop an old process
//! or fence its arbitrary external API effects. Callers must provide a consistent
//! checkpoint boundary and keep transient execution credentials outside this record.

use crate::{checkpoint::CheckpointRef, domain::*, Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

/// One non-secret placement configuration, in priority order in the session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryCandidate {
    /// Registered location; distinct configurations require distinct provider IDs.
    pub provider: ProviderId,
    /// Provider-owned configuration, never credentials.
    pub config: ProviderOptions,
    /// Provider-facing allocation requirements.
    pub resources: ResourceRequirements,
}

/// A complete immutable checkpoint selected as a recovery point.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryHead {
    /// Monotonic publication generation, starting at zero.
    pub generation: u64,
    /// Immutable artifact whose availability the application confirms before publication.
    pub checkpoint: CheckpointRef,
}

/// Progress of a pinned attempt. Later phases never repeat checkpoint restore.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecoveryPhase {
    /// Identity and checkpoint are durable before workspace side effects.
    Preparing,
    /// The pinned checkpoint was restored to this workspace.
    Restored {
        /// Exact provider reference observed during preparation.
        workspace: WorkspaceRef,
    },
    /// Stable launch intent is durable before requesting the native launch claim.
    LaunchRequested {
        /// Previously restored target, unchanged across retries.
        workspace: WorkspaceRef,
        /// Non-secret fingerprint of launch inputs; never an execution request or credentials.
        launch_identity: String,
    },
    /// Native launch claim was accepted; this does not promise workload success.
    Started {
        /// Reconnectable process reference returned for the pinned launch.
        run: RunRef,
        /// Same non-secret launch fingerprint recorded before launch.
        launch_identity: String,
    },
}

/// Durable intent for one epoch; uncertain effects retain these exact identities.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryAttempt {
    /// Stable switch request identity; older replaced requests cannot become new work.
    pub request_id: OperationId,
    /// Epoch observed by the caller before this attempt was created; zero initially.
    pub expected_previous_epoch: u64,
    /// Checked successor of the previous epoch; starts at one.
    pub epoch: u64,
    /// Selected registered provider; pinned before opening a workspace.
    pub provider: ProviderId,
    /// Deterministic fresh workspace identity for this session and epoch.
    pub workspace_id: WorkspaceId,
    /// Deterministic run identity reused for native launch retries.
    pub run_id: RunId,
    /// Complete committed head pinned before any restore or launch.
    pub checkpoint: RecoveryHead,
    /// Last durably recorded phase or launch intent.
    pub phase: RecoveryPhase,
}

/// Authoritative recovery entity, replaced atomically by repository revision.
///
/// Configuration and session identity never change. Advancing an epoch fences
/// canonical publication only; callers must separately address unfenced old work.
/// Serialization deliberately contains no transient `ExecutionRequest`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoverySession {
    /// Stable repository identity.
    pub id: OperationId,
    /// Non-secret application configuration fingerprint, immutable across retries.
    pub identity: String,
    /// Nonempty priority-ordered unique provider configurations.
    pub candidates: Vec<RecoveryCandidate>,
    /// CAS revision; creation starts at zero and every replacement increments once.
    pub revision: u64,
    /// Latest complete canonical checkpoint.
    pub committed: RecoveryHead,
    /// Pinned current attempt, retained even when its process outcome is uncertain.
    pub current_attempt: Option<RecoveryAttempt>,
}

impl RecoverySession {
    /// Validate a loaded snapshot, including reference and pinned lineage relationships.
    /// Artifact existence and launch-fingerprint authenticity remain application checks.
    pub fn validate(&self) -> Result<()> {
        if self.identity.is_empty() || self.candidates.is_empty() {
            return invalid("recovery identity and candidates must be nonempty");
        }
        let mut providers = HashSet::new();
        if self
            .candidates
            .iter()
            .any(|candidate| !providers.insert(&candidate.provider))
        {
            return invalid("recovery providers must be unique");
        }
        validate_head(&self.committed)?;
        if let Some(attempt) = &self.current_attempt {
            validate_head(&attempt.checkpoint)?;
            if checked_next(attempt.expected_previous_epoch)? != attempt.epoch
                || !providers.contains(&attempt.provider)
                || attempt.checkpoint.generation > self.committed.generation
                || (attempt.checkpoint.generation == self.committed.generation
                    && attempt.checkpoint != self.committed)
            {
                return invalid("recovery attempt has inconsistent epoch, provider or checkpoint");
            }
            let (workspace_id, run_id) = Self::attempt_ids(&self.id, attempt.epoch)?;
            if attempt.workspace_id != workspace_id || attempt.run_id != run_id {
                return invalid("recovery attempt identities do not match session and epoch");
            }
            if let Some(workspace) = workspace_of(&attempt.phase) {
                if workspace.version != 1
                    || workspace.id != attempt.workspace_id
                    || workspace.provider != attempt.provider
                {
                    return invalid("recovery workspace reference does not match pinned target");
                }
            }
            if let RecoveryPhase::Started { run, .. } = &attempt.phase {
                if run.id != attempt.run_id {
                    return invalid("recovery run reference does not match pinned run");
                }
            }
            if launch_identity(&attempt.phase).is_some_and(str::is_empty) {
                return invalid("recovery launch identity must be nonempty");
            }
            if attempt.checkpoint.generation < self.committed.generation
                && !matches!(attempt.phase, RecoveryPhase::Started { .. })
            {
                return invalid("only a started attempt can publish a newer checkpoint");
            }
        }
        if self.current_attempt.is_none() && self.committed.generation != 0 {
            return invalid("recovery head cannot advance without an attempt");
        }
        Ok(())
    }

    /// Validate creation or one atomic replacement of the previous canonical snapshot.
    /// Repositories must call this under the same short lock as their revision check.
    pub fn validate_replacement(&self, previous: Option<&Self>) -> Result<()> {
        self.validate()?;
        let Some(previous) = previous else {
            if self.revision != 0
                || self.committed.generation != 0
                || self.current_attempt.is_some()
            {
                return conflict(
                    "recovery creation must start at revision/generation zero without an attempt",
                );
            }
            return Ok(());
        };
        previous.validate()?;
        if self.id != previous.id
            || self.identity != previous.identity
            || self.candidates != previous.candidates
            || self.revision != previous.next_revision()?
        {
            return conflict(
                "recovery identity/configuration changed or revision is not its successor",
            );
        }
        let same_head = self.committed == previous.committed;
        match (&previous.current_attempt, &self.current_attempt) {
            (None, None) if same_head => Ok(()),
            (old, Some(new)) if old.as_ref().is_none_or(|old| old.epoch != new.epoch) => {
                if !same_head
                    || new.epoch != previous.next_epoch()?
                    || new.checkpoint != previous.committed
                    || !matches!(new.phase, RecoveryPhase::Preparing)
                    || old
                        .as_ref()
                        .is_some_and(|old| !matches!(old.phase, RecoveryPhase::Started { .. }))
                    || old
                        .as_ref()
                        .is_some_and(|old| old.request_id == new.request_id)
                {
                    return conflict(
                        "new recovery attempt must pin the current head and successor epoch",
                    );
                }
                Ok(())
            }
            (Some(old), Some(new)) => {
                if old.request_id != new.request_id
                    || old.expected_previous_epoch != new.expected_previous_epoch
                    || old.provider != new.provider
                    || old.workspace_id != new.workspace_id
                    || old.run_id != new.run_id
                    || old.checkpoint != new.checkpoint
                {
                    return conflict(
                        "recovery attempt identities and pinned checkpoint are immutable",
                    );
                }
                validate_phase_transition(&old.phase, &new.phase)?;
                if !same_head
                    && (self.committed.generation != checked_next(previous.committed.generation)?
                        || self.committed.checkpoint == previous.committed.checkpoint
                        || !matches!(old.phase, RecoveryPhase::Started { .. })
                        || !same_phase(&old.phase, &new.phase))
                {
                    return conflict("checkpoint publication requires an unchanged started attempt and successor generation");
                }
                Ok(())
            }
            _ => conflict("recovery attempts cannot be erased"),
        }
    }

    /// Checked successor revision; overflow refuses publication without wrapping.
    pub fn next_revision(&self) -> Result<u64> {
        checked_next(self.revision)
    }

    /// Checked next epoch, or one before the first attempt.
    pub fn next_epoch(&self) -> Result<u64> {
        checked_next(
            self.current_attempt
                .as_ref()
                .map_or(0, |attempt| attempt.epoch),
        )
    }

    /// Stable isolated target identities derived from the full session ID and epoch.
    pub fn attempt_ids(id: &OperationId, epoch: u64) -> Result<(WorkspaceId, RunId)> {
        if epoch == 0 {
            return invalid("recovery attempt epoch must be positive");
        }
        let digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&("recovery", id, epoch))?)
        );
        Ok((
            WorkspaceId::new(format!("recovery-{digest}"))?,
            RunId::new(format!("continuation-{digest}"))?,
        ))
    }
}

fn checked_next(value: u64) -> Result<u64> {
    value
        .checked_add(1)
        .ok_or_else(|| Error::Conflict("recovery counter exhausted".into()))
}
fn validate_head(head: &RecoveryHead) -> Result<()> {
    let value = &head.checkpoint.0;
    if value.is_empty()
        || value.len() > 80
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return invalid("invalid recovery checkpoint reference");
    }
    Ok(())
}
fn workspace_of(phase: &RecoveryPhase) -> Option<&WorkspaceRef> {
    match phase {
        RecoveryPhase::Preparing => None,
        RecoveryPhase::Restored { workspace }
        | RecoveryPhase::LaunchRequested { workspace, .. } => Some(workspace),
        RecoveryPhase::Started { run, .. } => Some(&run.workspace),
    }
}
fn launch_identity(phase: &RecoveryPhase) -> Option<&str> {
    match phase {
        RecoveryPhase::LaunchRequested {
            launch_identity, ..
        }
        | RecoveryPhase::Started {
            launch_identity, ..
        } => Some(launch_identity),
        _ => None,
    }
}
fn phase_rank(phase: &RecoveryPhase) -> u8 {
    match phase {
        RecoveryPhase::Preparing => 0,
        RecoveryPhase::Restored { .. } => 1,
        RecoveryPhase::LaunchRequested { .. } => 2,
        RecoveryPhase::Started { .. } => 3,
    }
}
fn same_phase(left: &RecoveryPhase, right: &RecoveryPhase) -> bool {
    phase_rank(left) == phase_rank(right)
        && workspace_of(left) == workspace_of(right)
        && launch_identity(left) == launch_identity(right)
        && match (left, right) {
            (
                RecoveryPhase::Started { run: left, .. },
                RecoveryPhase::Started { run: right, .. },
            ) => left.id == right.id,
            _ => true,
        }
}
fn validate_phase_transition(old: &RecoveryPhase, new: &RecoveryPhase) -> Result<()> {
    let old_rank = phase_rank(old);
    let new_rank = phase_rank(new);
    if new_rank == old_rank && same_phase(old, new) {
        return Ok(());
    }
    if new_rank != old_rank + 1
        || workspace_of(old).is_some_and(|workspace| Some(workspace) != workspace_of(new))
        || launch_identity(old).is_some_and(|identity| Some(identity) != launch_identity(new))
    {
        return conflict(
            "recovery phase cannot skip, regress or rebind observed references/launch identity",
        );
    }
    Ok(())
}
fn invalid<T>(message: &str) -> Result<T> {
    Err(Error::InvalidInput(message.into()))
}
fn conflict<T>(message: &str) -> Result<T> {
    Err(Error::Conflict(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> RecoverySession {
        RecoverySession {
            id: OperationId::new("portable-project").unwrap(),
            identity: "recipe-v1".into(),
            candidates: vec![RecoveryCandidate {
                provider: ProviderId::new("laptop").unwrap(),
                config: ProviderOptions::default(),
                resources: ResourceRequirements::default(),
            }],
            revision: 0,
            committed: RecoveryHead {
                generation: 0,
                checkpoint: CheckpointRef("seed".into()),
            },
            current_attempt: None,
        }
    }
    fn preparing(previous: &RecoverySession, request: &str) -> RecoverySession {
        let mut next = previous.clone();
        next.revision = previous.next_revision().unwrap();
        let epoch = previous.next_epoch().unwrap();
        let (workspace_id, run_id) = RecoverySession::attempt_ids(&next.id, epoch).unwrap();
        next.current_attempt = Some(RecoveryAttempt {
            request_id: OperationId::new(request).unwrap(),
            expected_previous_epoch: epoch - 1,
            epoch,
            provider: next.candidates[0].provider.clone(),
            workspace_id,
            run_id,
            checkpoint: previous.committed.clone(),
            phase: RecoveryPhase::Preparing,
        });
        next
    }
    fn workspace(session: &RecoverySession) -> WorkspaceRef {
        let attempt = session.current_attempt.as_ref().unwrap();
        WorkspaceRef {
            version: 1,
            id: attempt.workspace_id.clone(),
            provider: attempt.provider.clone(),
            locator: WorkspaceLocator::from_locator(&"observed-target").unwrap(),
        }
    }
    fn advance(previous: &RecoverySession, phase: RecoveryPhase) -> RecoverySession {
        let mut next = previous.clone();
        next.revision += 1;
        next.current_attempt.as_mut().unwrap().phase = phase;
        next
    }
    fn started() -> RecoverySession {
        let preparing = preparing(&seed(), "switch-1");
        let workspace = workspace(&preparing);
        let restored = advance(
            &preparing,
            RecoveryPhase::Restored {
                workspace: workspace.clone(),
            },
        );
        let requested = advance(
            &restored,
            RecoveryPhase::LaunchRequested {
                workspace: workspace.clone(),
                launch_identity: "command-digest".into(),
            },
        );
        advance(
            &requested,
            RecoveryPhase::Started {
                run: RunRef {
                    workspace,
                    id: requested.current_attempt.as_ref().unwrap().run_id.clone(),
                },
                launch_identity: "command-digest".into(),
            },
        )
    }

    #[test]
    fn valid_phases_head_publication_and_fresh_attempt_preserve_lineage() {
        let seed = seed();
        seed.validate_replacement(None).unwrap();
        let initial = preparing(&seed, "switch-1");
        initial.validate_replacement(Some(&seed)).unwrap();
        let target = workspace(&initial);
        let restored = advance(
            &initial,
            RecoveryPhase::Restored {
                workspace: target.clone(),
            },
        );
        restored.validate_replacement(Some(&initial)).unwrap();
        let requested = advance(
            &restored,
            RecoveryPhase::LaunchRequested {
                workspace: target.clone(),
                launch_identity: "command-digest".into(),
            },
        );
        requested.validate_replacement(Some(&restored)).unwrap();
        let started = advance(
            &requested,
            RecoveryPhase::Started {
                run: RunRef {
                    workspace: target,
                    id: requested.current_attempt.as_ref().unwrap().run_id.clone(),
                },
                launch_identity: "command-digest".into(),
            },
        );
        started.validate_replacement(Some(&requested)).unwrap();
        let mut published = started.clone();
        published.revision += 1;
        published.committed = RecoveryHead {
            generation: 1,
            checkpoint: CheckpointRef("complete-next".into()),
        };
        published.validate_replacement(Some(&started)).unwrap();
        assert_eq!(
            published.current_attempt.as_ref().unwrap().checkpoint,
            seed.committed
        );
        let next = preparing(&published, "switch-2");
        next.validate_replacement(Some(&published)).unwrap();
        assert_ne!(
            next.current_attempt.as_ref().unwrap().workspace_id,
            started.current_attempt.as_ref().unwrap().workspace_id
        );
        assert_eq!(
            next.current_attempt.as_ref().unwrap().checkpoint,
            published.committed
        );
        let encoded = serde_json::to_vec(&published).unwrap();
        serde_json::from_slice::<RecoverySession>(&encoded)
            .unwrap()
            .validate()
            .unwrap();
    }

    #[test]
    fn rejects_contradictory_snapshots_before_provider_effects() {
        let valid = started();
        let mut wrong = valid.clone();
        wrong.candidates.push(wrong.candidates[0].clone());
        assert!(wrong.validate().is_err());
        let mut wrong = valid.clone();
        wrong.current_attempt.as_mut().unwrap().provider = ProviderId::new("unknown").unwrap();
        assert!(wrong.validate().is_err());
        let mut wrong = valid.clone();
        wrong.current_attempt.as_mut().unwrap().workspace_id = WorkspaceId::new("another").unwrap();
        assert!(wrong.validate().is_err());
        let mut wrong = valid.clone();
        if let RecoveryPhase::Started { run, .. } =
            &mut wrong.current_attempt.as_mut().unwrap().phase
        {
            run.id = RunId::new("another").unwrap();
        }
        assert!(wrong.validate().is_err());
        let mut wrong = valid.clone();
        if let RecoveryPhase::Started { run, .. } =
            &mut wrong.current_attempt.as_mut().unwrap().phase
        {
            run.workspace.provider = ProviderId::new("another").unwrap();
        }
        assert!(wrong.validate().is_err());
        let mut wrong = valid.clone();
        wrong
            .current_attempt
            .as_mut()
            .unwrap()
            .checkpoint
            .checkpoint = CheckpointRef("uncommitted".into());
        assert!(wrong.validate().is_err());
        let mut wrong = valid.clone();
        wrong
            .current_attempt
            .as_mut()
            .unwrap()
            .expected_previous_epoch = 1;
        assert!(wrong.validate().is_err());
        let mut wrong = valid;
        wrong.committed.checkpoint = CheckpointRef("../unsafe".into());
        assert!(wrong.validate().is_err());
    }

    #[test]
    fn rejects_phase_skips_regression_reference_and_launch_rebinding() {
        let original = preparing(&seed(), "switch-1");
        let started = started();
        let mut skipped = started.clone();
        skipped.revision = original.revision + 1;
        assert!(skipped.validate_replacement(Some(&original)).is_err());
        let regression = advance(&started, RecoveryPhase::Preparing);
        assert!(regression.validate_replacement(Some(&started)).is_err());
        let mut rebound = started.clone();
        rebound.revision += 1;
        if let RecoveryPhase::Started {
            launch_identity, ..
        } = &mut rebound.current_attempt.as_mut().unwrap().phase
        {
            *launch_identity = "different-command".into();
        }
        assert!(rebound.validate_replacement(Some(&started)).is_err());
        let mut rebound = started.clone();
        rebound.revision += 1;
        if let RecoveryPhase::Started { run, .. } =
            &mut rebound.current_attempt.as_mut().unwrap().phase
        {
            run.workspace.locator = WorkspaceLocator::from_locator(&"different-target").unwrap();
        }
        assert!(rebound.validate_replacement(Some(&started)).is_err());
        let mut erased = started.clone();
        erased.revision += 1;
        erased.current_attempt = None;
        assert!(erased.validate_replacement(Some(&started)).is_err());
    }

    #[test]
    fn rejects_stale_requests_configuration_rebinding_and_unsafe_head_changes() {
        let old = started();
        let mut same_request = preparing(&old, "switch-1");
        assert!(same_request.validate_replacement(Some(&old)).is_err());
        same_request.current_attempt.as_mut().unwrap().request_id =
            OperationId::new("switch-2").unwrap();
        same_request.validate_replacement(Some(&old)).unwrap();
        let mut changed = same_request.clone();
        changed.identity = "new-config".into();
        assert!(changed.validate_replacement(Some(&old)).is_err());
        let mut changed = same_request;
        changed
            .current_attempt
            .as_mut()
            .unwrap()
            .checkpoint
            .checkpoint = CheckpointRef("uncommitted".into());
        assert!(changed.validate_replacement(Some(&old)).is_err());
        let mut published = old.clone();
        published.revision += 1;
        published.committed.generation += 2;
        published.committed.checkpoint = CheckpointRef("new".into());
        assert!(published.validate_replacement(Some(&old)).is_err());
        let preparing = preparing(&seed(), "switch-1");
        published = preparing.clone();
        published.revision += 1;
        published.committed.generation = 1;
        published.committed.checkpoint = CheckpointRef("new".into());
        assert!(published.validate_replacement(Some(&preparing)).is_err());
    }

    #[test]
    fn unfinished_or_uncertain_attempt_cannot_be_replaced() {
        let initial = preparing(&seed(), "switch-1");
        let restored = advance(
            &initial,
            RecoveryPhase::Restored {
                workspace: workspace(&initial),
            },
        );
        let requested = advance(
            &restored,
            RecoveryPhase::LaunchRequested {
                workspace: workspace(&restored),
                launch_identity: "command-digest".into(),
            },
        );
        for previous in [&initial, &restored, &requested] {
            let replacement = preparing(previous, "switch-2");
            assert!(matches!(
                replacement.validate_replacement(Some(previous)),
                Err(Error::Conflict(_))
            ));
        }
    }

    #[test]
    fn creation_revision_and_configuration_are_strict() {
        let initial = seed();
        let mut invalid = initial.clone();
        invalid.revision = 1;
        assert!(invalid.validate_replacement(None).is_err());
        assert!(preparing(&initial, "switch-1")
            .validate_replacement(None)
            .is_err());
        let mut invalid = initial.clone();
        invalid.candidates.clear();
        assert!(invalid.validate().is_err());
        let mut replacement = preparing(&initial, "switch-1");
        replacement.revision = initial.revision;
        assert!(replacement.validate_replacement(Some(&initial)).is_err());
        let mut replacement = preparing(&initial, "switch-1");
        replacement.candidates[0].config = ProviderOptions::from_options(&"different").unwrap();
        assert!(replacement.validate_replacement(Some(&initial)).is_err());
        let mut replacement = preparing(&initial, "switch-1");
        replacement.current_attempt.as_mut().unwrap().run_id = RunId::new("different").unwrap();
        assert!(replacement.validate().is_err());
        let mut replacement = started();
        if let RecoveryPhase::Started {
            launch_identity, ..
        } = &mut replacement.current_attempt.as_mut().unwrap().phase
        {
            launch_identity.clear();
        }
        assert!(replacement.validate().is_err());
        let mut wire = serde_json::to_value(initial).unwrap();
        wire["execution"] = serde_json::json!({"secret": "must-not-persist"});
        assert!(serde_json::from_value::<RecoverySession>(wire).is_err());
    }

    #[test]
    fn checked_counters_never_wrap_and_ids_are_stable_and_isolated() {
        let mut session = started();
        session.revision = u64::MAX;
        assert!(session.next_revision().is_err());
        session.current_attempt.as_mut().unwrap().epoch = u64::MAX;
        assert!(session.next_epoch().is_err());
        assert!(checked_next(u64::MAX).is_err());
        let id = OperationId::new("session").unwrap();
        let first = RecoverySession::attempt_ids(&id, 1).unwrap();
        assert_eq!(first, RecoverySession::attempt_ids(&id, 1).unwrap());
        assert_ne!(first, RecoverySession::attempt_ids(&id, 2).unwrap());
        assert_ne!(
            first,
            RecoverySession::attempt_ids(&OperationId::new("session-1").unwrap(), 1).unwrap()
        );
        assert!(RecoverySession::attempt_ids(&id, 0).is_err());
    }
}
