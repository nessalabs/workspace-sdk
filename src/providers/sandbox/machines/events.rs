//! What the machines tell a caller's own records. No event holds a
//! credential, a provider body, a variable's value or a value from a
//! rejected receipt.
use super::diagnostics::Diagnostic;

/// Why a caller stops a VM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StopCause {
    /// Nothing used it for the caller's idle time.
    Idle,
    /// Its work was closed.
    Closed,
    /// It was woken to be checked or used and could not be.
    NotReady,
    /// It was refused, or its check did not finish.
    NotAdmitted,
}
impl StopCause {
    /// Its name in a record.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Closed => "closed",
            Self::NotReady => "not_ready",
            Self::NotAdmitted => "not_admitted",
        }
    }
}

/// What happened, for the caller's own records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MachineEvent {
    /// A receipt was refused at the [`Diagnostic`]'s place.
    ReceiptRejected(Diagnostic),
    /// A VM's receipt said its inherited credentials were still being
    /// wiped; `startup_phase` is `operable`, `starting` or `other`.
    CredentialWipePending { startup_phase: &'static str },
    /// A VM is about to be stopped, its disk kept.
    StopRequested { vm: String, cause: StopCause },
    /// What Boat answered that stop; `failure` is a
    /// [`MachineFailure::kind`](super::MachineFailure::kind).
    StopAnswered {
        vm: String,
        cause: StopCause,
        failure: Option<&'static str>,
    },
    /// A rollback of a new VM was asked; `confirmed` is false unless Boat
    /// confirmed the deletion, which it never does today.
    RollbackObserved { vm: String, confirmed: bool },
}

/// The caller's sink for [`MachineEvent`]s.
pub trait IMachineEvents: Send + Sync {
    /// Keep `event`. It returns nothing, so a failing sink never changes an
    /// operation's outcome; it must not block, since it runs inline. A
    /// panic in it is caught and dropped, except in a build with
    /// `panic = "abort"`, where it ends the process.
    fn record(&self, event: MachineEvent);
}
