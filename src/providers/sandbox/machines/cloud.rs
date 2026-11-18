//! Boat as one of a caller's clouds: [`BoatMachines`] behind the
//! provider-neutral [`IMachines`] port.
//!
//! ```text
//! MachineRequest + (environment, machine type) ─► MachineSpec
//! create ─► Created ─► Made { proven: "boat:<env id>:<version id>:<version>:<env name>" }
//! ready(proven) ─► EnvironmentIdentity ─► BoatMachines::ready
//! remove ─► Created::restore(RollbackNewVm) ─► roll_back
//! ```
//!
//! Boat's backstop is a hard lifetime: a [`Lifetime::Idle`] is refused.
use super::{
    account::{BoatMachines, Created, Disposition as BoatDisposition},
    environment::{AdmissionRefusal, AdmissionRejection, EnvironmentIdentity},
    events::StopCause,
    spec::{MachineFailure, MachineSpec},
};
use crate::{
    domain::machines::{
        Backstop, Disposition, Lifetime, MachineError, MachineRequest, Made, Proven, RefusalCause,
        Refused, StopReason,
    },
    ports::{removable, IMachines},
};
use async_trait::async_trait;

const PROVEN_PREFIX: &str = "boat";

/// One Boat account's VMs, made in one environment at one size, as
/// [`IMachines`].
#[derive(Clone)]
pub struct BoatCloud {
    machines: BoatMachines,
    environment: String,
    machine_type: String,
}
impl BoatCloud {
    /// `machines`' VMs, each made in the account environment
    /// `environment`, of Boat's size `machine_type` (such as `small`).
    ///
    /// # Errors
    /// [`MachineError::InvalidInput`] unless both are one plain segment, as
    /// [`MachineSpec::new`] takes them.
    pub fn new(
        machines: BoatMachines,
        environment: impl Into<String>,
        machine_type: impl Into<String>,
    ) -> Result<Self, MachineError> {
        let cloud = Self {
            machines,
            environment: environment.into(),
            machine_type: machine_type.into(),
        };
        MachineSpec::new(
            "image",
            &*cloud.environment,
            &*cloud.machine_type,
            None,
            Default::default(),
        )
        .map_err(error)?;
        Ok(cloud)
    }

    /// Boat's spec for `request`.
    fn spec(&self, request: &MachineRequest) -> Result<MachineSpec, MachineError> {
        let lifetime = match request.lifetime() {
            Lifetime::Unlimited => None,
            // Rounded up, so Boat never stops one sooner than asked.
            Lifetime::Hard(duration) => Some(
                u32::try_from(duration.as_secs() + u64::from(duration.subsec_nanos() > 0))
                    .map_err(|_| MachineError::InvalidInput)?,
            ),
            Lifetime::Idle(_) => return Err(MachineError::InvalidInput),
        };
        MachineSpec::new(
            request.template(),
            &*self.environment,
            &*self.machine_type,
            lifetime,
            request.variables().clone(),
        )
        .map_err(error)
    }
}

#[async_trait]
impl IMachines for BoatCloud {
    fn backstop(&self) -> Backstop {
        Backstop::HardLifetime
    }
    async fn prepare(&self, request: &MachineRequest) -> Result<(), MachineError> {
        self.machines
            .prepare(&self.spec(request)?)
            .await
            .map_err(error)
    }
    async fn create(&self, request: &MachineRequest, key: &str) -> Result<Made, MachineError> {
        let created = self
            .machines
            .create(&self.spec(request)?, key)
            .await
            .map_err(error)?;
        Ok(Made::new(
            created.vm().to_owned(),
            proven(created.admission())?,
            created.refusal().map(refused),
            match created.disposition() {
                BoatDisposition::RollbackNewVm => Disposition::Remove,
                BoatDisposition::RetainTarget => Disposition::Retain,
            },
        ))
    }
    async fn ready(
        &self,
        request: &MachineRequest,
        machine: &str,
        proven: &Proven,
    ) -> Result<(), MachineError> {
        let identity = identity(proven)?;
        self.machines
            .ready(&self.spec(request)?, machine, &identity)
            .await
            .map(drop)
            .map_err(error)
    }
    async fn stop(&self, machine: &str, reason: StopReason) -> Result<(), MachineError> {
        let cause = match reason {
            StopReason::Idle => StopCause::Idle,
            StopReason::Closed => StopCause::Closed,
            StopReason::NotReady => StopCause::NotReady,
            StopReason::NotAdmitted => StopCause::NotAdmitted,
        };
        self.machines.stop(machine, cause).await.map_err(error)
    }
    async fn remove(&self, made: &Made) -> Result<(), MachineError> {
        removable(made)?;
        let created = Created::restore(
            made.machine().to_owned(),
            identity(made.proven())?,
            BoatDisposition::RollbackNewVm,
        )
        .map_err(error)?;
        self.machines.roll_back(&created).await.map_err(error)
    }
}

/// What a VM was admitted under, as a proof: Boat reports no digest.
fn proven(identity: &EnvironmentIdentity) -> Result<Proven, MachineError> {
    Proven::new(
        format!(
            "{PROVEN_PREFIX}:{}:{}:{}:{}",
            identity.id(),
            identity.version_id(),
            identity.version(),
            identity.name()
        ),
        None,
    )
}

/// The environment identity a Boat proof names.
///
/// # Errors
/// [`MachineError::InvalidInput`] for a proof that is not Boat's.
fn identity(proven: &Proven) -> Result<EnvironmentIdentity, MachineError> {
    let mut parts = proven.identity().splitn(5, ':');
    let (Some(PROVEN_PREFIX), Some(id), Some(version_id), Some(version), Some(name), None) = (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        proven.digest(),
    ) else {
        return Err(MachineError::InvalidInput);
    };
    let version = version.parse().map_err(|_| MachineError::InvalidInput)?;
    EnvironmentIdentity::new(id.into(), name.into(), version_id.into(), version)
        .map_err(|_| MachineError::InvalidInput)
}

fn refused(rejection: &AdmissionRejection) -> Refused {
    let cause = rejection.cause();
    Refused {
        cause: match cause {
            AdmissionRefusal::MissingEnvironment
            | AdmissionRefusal::WrongEnvironment
            | AdmissionRefusal::MissingVersion
            | AdmissionRefusal::WrongVersion => RefusalCause::WrongImage,
            AdmissionRefusal::EnvironmentChanged => RefusalCause::Changed,
            AdmissionRefusal::EnvironmentUnavailable
            | AdmissionRefusal::InvalidReceipt
            | AdmissionRefusal::ReceiptUnavailable => RefusalCause::Unproven,
            AdmissionRefusal::LifetimeNotEnforced => RefusalCause::LifetimeNotEnforced,
            AdmissionRefusal::WipePending => RefusalCause::Pending,
        },
        detail: cause.as_str(),
    }
}

fn error(failure: MachineFailure) -> MachineError {
    match failure {
        MachineFailure::InvalidInput => MachineError::InvalidInput,
        MachineFailure::Provider => MachineError::Provider,
        MachineFailure::Missing => MachineError::Missing,
        MachineFailure::EnvironmentUnavailable => MachineError::EnvironmentUnavailable,
        MachineFailure::AdmissionRejected(rejection) => {
            MachineError::AdmissionRejected(refused(&rejection))
        }
        MachineFailure::ProvisioningUncertain => MachineError::ProvisioningUncertain,
        MachineFailure::CleanupUncertain => MachineError::CleanupUncertain,
    }
}

#[cfg(test)]
#[path = "cloud_tests.rs"]
mod tests;
