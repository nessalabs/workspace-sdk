#![doc = include_str!("../../docs/handoff.md")]

mod file;
#[cfg(unix)]
pub use file::FileReceiptStore;

use serde::{Deserialize, Serialize};
use std::{fmt, future::Future};

pub type HandoffResult<T> = std::result::Result<T, HandoffError>;

/// Errors from receipt validation, concurrency control, or persistence.
#[derive(thiserror::Error)]
pub enum HandoffError {
    #[error("invalid handoff id (use 1-96 ASCII letters, digits, underscores or hyphens)")]
    InvalidId,
    #[error("handoff plan and references must not be empty")]
    InvalidInput,
    #[error("handoff receipt has changed; reload and reconcile before continuing")]
    Conflict,
    #[error("handoff id already belongs to a different plan")]
    PlanMismatch,
    #[error("handoff receipt was not found")]
    NotFound,
    #[error("invalid handoff transition from {0:?}")]
    InvalidTransition(HandoffPhase),
    #[error("handoff receipt is corrupt or uses an unsupported schema")]
    Corrupt,
    #[error("handoff receipt exceeds configured size limit")]
    TooLarge,
    #[error("handoff receipt is locked by another operation; reload before retrying")]
    Busy,
    #[error("handoff storage failed (details redacted); reload to determine whether the write committed")]
    Storage(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl HandoffError {
    /// Wrap a backend error without exposing its details in ordinary logs.
    pub fn storage(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Storage(Box::new(error))
    }
}
impl From<std::io::Error> for HandoffError {
    fn from(error: std::io::Error) -> Self {
        Self::storage(error)
    }
}
impl fmt::Debug for HandoffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// The immutable identity of an application-managed transfer.
///
/// `intent` should be a stable digest or reference covering the complete intended
/// work (workspace, exclusions, continuation inputs, etc.). Reusing an operation
/// id with a different plan is rejected. Do not put credentials in this record.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffPlan {
    pub source_sandbox_id: String,
    pub target_sandbox_id: String,
    pub intent: String,
}
impl fmt::Debug for HandoffPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HandoffPlan { fields: <redacted> }")
    }
}
impl HandoffPlan {
    fn validate(&self) -> HandoffResult<()> {
        if [
            &self.source_sandbox_id,
            &self.target_sandbox_id,
            &self.intent,
        ]
        .iter()
        .any(|v| v.trim().is_empty())
        {
            return Err(HandoffError::InvalidInput);
        }
        Ok(())
    }
}

/// Reference to an immutable checkpoint managed by the application.
/// The SDK stores these values; it does not fetch the data or verify its digest.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointReference {
    pub reference: String,
    pub digest: Option<String>,
}
impl fmt::Debug for CheckpointReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CheckpointReference { fields: <redacted> }")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffPhase {
    Prepared,
    Capturing,
    Captured,
    Restoring,
    Restored,
    Starting,
    Started,
}
impl HandoffPhase {
    /// When reloading one of these phases, the external action's outcome is unknown.
    /// A fresh successful Begin transition grants its caller permission to act once.
    pub fn needs_reconciliation(self) -> bool {
        matches!(self, Self::Capturing | Self::Restoring | Self::Starting)
    }
}

/// A versioned receipt. Read-only fields prevent accidental changes to its identity.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffReceipt {
    schema_version: u32,
    id: String,
    revision: u64,
    plan: HandoffPlan,
    phase: HandoffPhase,
    checkpoint: Option<CheckpointReference>,
    run_id: Option<String>,
    last_reconciliation: Option<String>,
}
impl fmt::Debug for HandoffReceipt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HandoffReceipt")
            .field("revision", &self.revision)
            .field("phase", &self.phase)
            .finish_non_exhaustive()
    }
}
impl HandoffReceipt {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn plan(&self) -> &HandoffPlan {
        &self.plan
    }
    pub fn phase(&self) -> HandoffPhase {
        self.phase
    }
    pub fn checkpoint(&self) -> Option<&CheckpointReference> {
        self.checkpoint.as_ref()
    }
    pub fn run_id(&self) -> Option<&str> {
        self.run_id.as_deref()
    }
    /// Latest application-supplied reference to evidence that a pending action did not apply.
    pub fn last_reconciliation(&self) -> Option<&str> {
        self.last_reconciliation.as_deref()
    }

    fn validate(&self) -> HandoffResult<()> {
        validate_id(&self.id).map_err(|_| HandoffError::Corrupt)?;
        self.plan.validate().map_err(|_| HandoffError::Corrupt)?;
        let has_checkpoint = matches!(
            self.phase,
            HandoffPhase::Captured
                | HandoffPhase::Restoring
                | HandoffPhase::Restored
                | HandoffPhase::Starting
                | HandoffPhase::Started
        );
        if self.schema_version != 1
            || self.checkpoint.is_some() != has_checkpoint
            || self.run_id.is_some() != (self.phase == HandoffPhase::Started)
            || self.checkpoint.as_ref().is_some_and(|c| {
                c.reference.trim().is_empty()
                    || c.digest.as_ref().is_some_and(|d| d.trim().is_empty())
            })
            || self.run_id.as_ref().is_some_and(|id| id.trim().is_empty())
            || self
                .last_reconciliation
                .as_ref()
                .is_some_and(|r| r.trim().is_empty())
        {
            return Err(HandoffError::Corrupt);
        }
        Ok(())
    }

    fn next(&self, transition: HandoffTransition) -> HandoffResult<Self> {
        self.validate()?;
        let mut next = self.clone();
        use HandoffPhase as P;
        use HandoffTransition as T;
        next.phase = match (self.phase, transition) {
            (P::Prepared, T::BeginCapture) => P::Capturing,
            (P::Capturing, T::Captured(checkpoint)) => {
                if checkpoint.reference.trim().is_empty()
                    || checkpoint
                        .digest
                        .as_ref()
                        .is_some_and(|d| d.trim().is_empty())
                {
                    return Err(HandoffError::InvalidInput);
                }
                next.checkpoint = Some(checkpoint);
                P::Captured
            }
            (P::Captured, T::BeginRestore) => P::Restoring,
            (P::Restoring, T::Restored) => P::Restored,
            (P::Restored, T::BeginStart) => P::Starting,
            (P::Starting, T::Started { run_id }) => {
                if run_id.trim().is_empty() {
                    return Err(HandoffError::InvalidInput);
                }
                next.run_id = Some(run_id);
                P::Started
            }
            (phase, T::ReconcileNotApplied { evidence_reference })
                if phase.needs_reconciliation() =>
            {
                if evidence_reference.trim().is_empty() {
                    return Err(HandoffError::InvalidInput);
                }
                next.last_reconciliation = Some(evidence_reference);
                match phase {
                    P::Capturing => P::Prepared,
                    P::Restoring => P::Captured,
                    P::Starting => P::Restored,
                    _ => unreachable!(),
                }
            }
            _ => return Err(HandoffError::InvalidTransition(self.phase)),
        };
        next.revision = next.revision.checked_add(1).ok_or(HandoffError::Corrupt)?;
        Ok(next)
    }
}

/// Begin transitions must commit before the application performs their action.
/// Confirmation transitions may also reconcile actions found to have completed.
/// Never reset an uncertain action based solely on a timeout or missing response.
#[derive(Clone)]
pub enum HandoffTransition {
    BeginCapture,
    Captured(CheckpointReference),
    BeginRestore,
    Restored,
    BeginStart,
    Started {
        run_id: String,
    },
    /// Application verified the old action cannot still complete and made any
    /// partial effects safe. Evidence is an opaque audit reference, not checked by the SDK.
    ReconcileNotApplied {
        evidence_reference: String,
    },
}

/// Pluggable durable storage. Implement with a database for distributed controllers.
///
/// `load` must return only durably committed receipts.
/// `compare_exchange` MUST atomically test the expected revision and durably commit
/// the new receipt; None means insert only if absent. On mismatch return Conflict.
/// A successful write must survive process restart. Never implement this as an
/// unlocked read followed by a write. Backend failures may have committed: callers
/// reload and reconcile. The store must not log receipt contents or credentials.
pub trait ReceiptStore: Send + Sync {
    fn load(&self, id: &str) -> impl Future<Output = HandoffResult<Option<HandoffReceipt>>> + Send;
    fn compare_exchange(
        &self,
        receipt: &HandoffReceipt,
        expected_revision: Option<u64>,
    ) -> impl Future<Output = HandoffResult<()>> + Send;
}

/// Coordinates receipts using a caller-selected storage backend. The application
/// implements capture, transfer, verification, and launch through callbacks or explicit transitions.
pub struct HandoffJournal<S> {
    store: S,
}
impl<S: ReceiptStore> HandoffJournal<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Create once, or return the existing receipt if the complete plan matches.
    pub async fn create(&self, id: &str, plan: HandoffPlan) -> HandoffResult<HandoffReceipt> {
        validate_id(id)?;
        plan.validate()?;
        let receipt = HandoffReceipt {
            schema_version: 1,
            id: id.into(),
            revision: 0,
            plan: plan.clone(),
            phase: HandoffPhase::Prepared,
            checkpoint: None,
            run_id: None,
            last_reconciliation: None,
        };
        match self.store.compare_exchange(&receipt, None).await {
            Ok(()) => {
                tracing::debug!(target: "boat_client::handoff", phase = ?receipt.phase, revision = receipt.revision, "receipt created");
                Ok(receipt)
            }
            Err(HandoffError::Conflict) => {
                let existing = self.load(id).await?;
                if existing.plan != plan {
                    return Err(HandoffError::PlanMismatch);
                }
                Ok(existing)
            }
            Err(error) => Err(error),
        }
    }

    pub async fn load(&self, id: &str) -> HandoffResult<HandoffReceipt> {
        validate_id(id)?;
        let receipt = self.store.load(id).await?.ok_or(HandoffError::NotFound)?;
        receipt.validate()?;
        if receipt.id != id {
            return Err(HandoffError::Corrupt);
        }
        tracing::debug!(target: "boat_client::handoff", phase = ?receipt.phase, revision = receipt.revision, "receipt loaded");
        Ok(receipt)
    }

    /// Commit capture intent, invoke the application once, and persist its checkpoint reference.
    /// An action error or cancellation leaves the receipt in-flight for reconciliation.
    pub async fn capture_with<F, Fut>(
        &self,
        receipt: &HandoffReceipt,
        capture: F,
    ) -> HandoffResult<HandoffReceipt>
    where
        F: FnOnce(HandoffReceipt) -> Fut,
        Fut: Future<Output = HandoffResult<CheckpointReference>>,
    {
        self.run_step(
            receipt,
            HandoffTransition::BeginCapture,
            |pending| async move { capture(pending).await.map(HandoffTransition::Captured) },
        )
        .await
    }

    /// Commit restore intent, invoke the application's transfer/verification, and confirm it.
    /// This does not automatically retry the application's callback.
    pub async fn restore_with<F, Fut>(
        &self,
        receipt: &HandoffReceipt,
        restore: F,
    ) -> HandoffResult<HandoffReceipt>
    where
        F: FnOnce(HandoffReceipt) -> Fut,
        Fut: Future<Output = HandoffResult<()>>,
    {
        self.run_step(
            receipt,
            HandoffTransition::BeginRestore,
            |pending| async move { restore(pending).await.map(|()| HandoffTransition::Restored) },
        )
        .await
    }

    /// Commit launch intent, invoke the application once, and persist its remote run id.
    /// A lost launch response requires reconciliation; never blindly call this again.
    pub async fn start_with<F, Fut>(
        &self,
        receipt: &HandoffReceipt,
        start: F,
    ) -> HandoffResult<HandoffReceipt>
    where
        F: FnOnce(HandoffReceipt) -> Fut,
        Fut: Future<Output = HandoffResult<String>>,
    {
        self.run_step(
            receipt,
            HandoffTransition::BeginStart,
            |pending| async move {
                start(pending)
                    .await
                    .map(|run_id| HandoffTransition::Started { run_id })
            },
        )
        .await
    }

    async fn run_step<F, Fut>(
        &self,
        receipt: &HandoffReceipt,
        begin: HandoffTransition,
        action: F,
    ) -> HandoffResult<HandoffReceipt>
    where
        F: FnOnce(HandoffReceipt) -> Fut,
        Fut: Future<Output = HandoffResult<HandoffTransition>>,
    {
        let pending = self.advance(receipt, begin).await?;
        let confirmation = action(pending.clone()).await?;
        self.advance(&pending, confirmation).await
    }

    /// Advance only if the stored revision still equals this receipt's revision.
    /// After any error or cancellation, reload before deciding whether to perform external work.
    /// Canceling a storage future does not guarantee its write was canceled.
    pub async fn advance(
        &self,
        receipt: &HandoffReceipt,
        transition: HandoffTransition,
    ) -> HandoffResult<HandoffReceipt> {
        let next = receipt.next(transition)?;
        if let Err(error) = self
            .store
            .compare_exchange(&next, Some(receipt.revision))
            .await
        {
            tracing::debug!(target: "boat_client::handoff", phase = ?receipt.phase, revision = receipt.revision,
                error_kind = handoff_error_kind(&error), "receipt write not confirmed; reload before continuing");
            return Err(error);
        }
        tracing::debug!(target: "boat_client::handoff", previous_phase = ?receipt.phase,
            phase = ?next.phase, revision = next.revision, "receipt transition committed");
        Ok(next)
    }
}

fn validate_id(id: &str) -> HandoffResult<()> {
    if id.is_empty()
        || id.len() > 96
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(HandoffError::InvalidId);
    }
    Ok(())
}

fn handoff_error_kind(error: &HandoffError) -> &'static str {
    match error {
        HandoffError::Conflict => "conflict",
        HandoffError::Busy => "busy",
        HandoffError::Storage(_) => "storage",
        HandoffError::Corrupt => "corrupt",
        HandoffError::TooLarge => "too_large",
        _ => "validation",
    }
}
