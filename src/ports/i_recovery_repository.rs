use super::IWorkflowLease;
use crate::{domain::*, Result};

/// Recovery authority with a separate whole-usecase effect lock and short CAS operations.
///
/// The nonexpiring effect lock serializes restore/launch across controllers; CAS
/// separately fences canonical publication, not arbitrary effects by old processes.
/// Implementations share workflow identity storage and reject cross-kind IDs.
pub trait IRecoveryRepository: Send + Sync {
    /// Nonblocking exclusive control across the entire prepare/restore/launch use case.
    ///
    /// Keep this existing workflow lease alive across side effects. It is not an
    /// expiring distributed lease: an unavailable controller cannot be stolen from
    /// while it still owns the lock. File repositories require a shared lock/store
    /// filesystem; local owner drop or process death releases their OS lock. This
    /// does not establish off-host authority durability or stop old target processes.
    fn acquire_recovery(&self, id: &OperationId) -> Result<Box<dyn IWorkflowLease>>;

    /// Load and validate a recovery snapshot; another workflow kind is a conflict.
    fn load_recovery(&self, id: &OperationId) -> Result<Option<RecoverySession>>;

    /// Atomically validate and install a canonical snapshot under a short lock.
    ///
    /// `None` creates an absent identity; `Some(revision)` replaces only that exact
    /// revision. A wrong key, existing other workflow kind, revision mismatch or
    /// invalid transition is refused; conflicts never permit remote side effects.
    /// Implementations call `RecoverySession::validate_replacement` before writing.
    fn compare_exchange_recovery(
        &self,
        id: &OperationId,
        expected_revision: Option<u64>,
        replacement: &RecoverySession,
    ) -> Result<()>;
}
