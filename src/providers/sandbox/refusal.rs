//! How a Boat machine's failures become SDK errors: a refusal only when the
//! cause can never pass, and an error with no outcome otherwise.
use super::locator::Machine;
use super::machines::{AdmissionRefusal, MachineFailure};
use crate::{domain::*, Error};

/// The refusal a cause is, when it is certain from the receipt and can never
/// pass later; `None` for a check that may pass when asked again.
pub(super) fn final_kind(cause: AdmissionRefusal) -> Option<RefusalKind> {
    match cause {
        AdmissionRefusal::MissingEnvironment
        | AdmissionRefusal::WrongEnvironment
        | AdmissionRefusal::MissingVersion
        | AdmissionRefusal::WrongVersion
        // A retry would read the new environment and find the VM's receipt
        // naming the old version, so this never passes either.
        | AdmissionRefusal::EnvironmentChanged => Some(RefusalKind::EnvironmentMismatch),
        AdmissionRefusal::EnvironmentUnavailable
        | AdmissionRefusal::InvalidReceipt
        | AdmissionRefusal::LifetimeNotEnforced => Some(RefusalKind::NotAdmitted),
        AdmissionRefusal::ReceiptUnavailable | AdmissionRefusal::WipePending => None,
    }
}

/// A machine failure with no refusal: the record stays pending, and asking
/// again with the same key finishes it.
pub(super) fn failure(failure: MachineFailure) -> Error {
    let provider = |message: &str| Error::Provider {
        provider: super::branding::NAME.into(),
        message: message.into(),
    };
    match failure {
        MachineFailure::InvalidInput => {
            Error::InvalidInput("Boat could not take the machine's spec or key".into())
        }
        MachineFailure::Missing => Error::NotFound(
            "Boat does not know the image, or the machine its key names is gone; \
             forget the unfinished workspace to start again"
                .into(),
        ),
        MachineFailure::EnvironmentUnavailable => Error::State(
            "the account's environment is ambiguous or passes more than agent credentials; \
             fix or delete it"
                .into(),
        ),
        MachineFailure::ProvisioningUncertain => {
            provider("Boat may have made the machine; asking again finishes it")
        }
        MachineFailure::CleanupUncertain => provider("Boat did not confirm a deletion"),
        MachineFailure::AdmissionRejected(rejection) => provider(&rejection.to_string()),
        MachineFailure::Provider => provider("Boat failed, refused or timed out"),
    }
}

/// A machine that failed its check on waking: refused when the cause is
/// final, left for the next wake otherwise. `ready` stopped it if it woke it.
pub(super) fn wake_failure(failed: MachineFailure, vm: &str, machine: &Machine) -> Error {
    let MachineFailure::AdmissionRejected(rejection) = failed else {
        return failure(failed);
    };
    match final_kind(rejection.cause()) {
        Some(kind) => Error::Refused(Box::new(Refusal {
            kind,
            message: rejection.to_string(),
            cleanup: RefusalCleanup::Kept,
            target: Some(vm.to_owned()),
            admission: Some(machine.admission()),
        })),
        None => Error::Provider {
            provider: super::branding::NAME.into(),
            message: format!("{rejection}; waking it again checks it again"),
        },
    }
}
