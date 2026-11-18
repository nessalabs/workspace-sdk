//! Agent37 as one of a caller's clouds: [`Agent37Machines`] behind the
//! provider-neutral [`IMachines`] port.
//!
//! ```text
//! MachineRequest (+ size) ─► MachineSpec
//! create ─► Created ─► Made { proven: "agent37:<template>[#<revision>]" + image digest }
//! ready(proven) ─► Admission ─► Agent37Machines::ready
//! remove ─► delete
//! ```
//!
//! Agent37's backstop is idle sleep, within [`IDLE`]: a [`Lifetime::Hard`]
//! is refused, and [`Lifetime::Unlimited`] sleeps after the longest idle
//! time, keeping its disk; [`IMachines::ready`] wakes it.
use super::{
    failure::{AdmissionRefusal, Agent37Failure},
    machines::{Admission, Agent37Machines, Created},
    spec::{MachineSpec, Size, IDLE},
};
use crate::{
    domain::machines::{
        Backstop, Disposition, Lifetime, MachineError, MachineRequest, Made, Proven, RefusalCause,
        Refused, StopReason,
    },
    ports::{removable, IMachines},
};
use async_trait::async_trait;

const PROVEN_PREFIX: &str = "agent37:";

/// One Agent37 workspace's instances, of one shape, as [`IMachines`].
#[derive(Clone, Debug)]
pub struct Agent37Cloud {
    machines: Agent37Machines,
    size: Option<Size>,
}
impl Agent37Cloud {
    /// `machines`' instances, of Agent37's smallest shape.
    pub fn new(machines: Agent37Machines) -> Self {
        Self {
            machines,
            size: None,
        }
    }
    /// The same instances in another shape.
    pub fn with_size(mut self, size: Size) -> Self {
        self.size = Some(size);
        self
    }

    /// Agent37's spec for `request`.
    fn spec(&self, request: &MachineRequest) -> Result<MachineSpec, MachineError> {
        let idle = match request.lifetime() {
            Lifetime::Unlimited => *IDLE.end(),
            Lifetime::Idle(idle) => idle,
            Lifetime::Hard(_) => return Err(MachineError::InvalidInput),
        };
        let spec = MachineSpec::new(request.template(), idle, request.variables().clone())
            .map_err(error)?;
        Ok(match self.size {
            Some(size) => spec.with_size(size),
            None => spec,
        })
    }
}

#[async_trait]
impl IMachines for Agent37Cloud {
    fn backstop(&self) -> Backstop {
        Backstop::IdleSleep {
            shortest: *IDLE.start(),
            longest: *IDLE.end(),
        }
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
        Ok(made(&created, request))
    }
    async fn ready(
        &self,
        _request: &MachineRequest,
        machine: &str,
        proven: &Proven,
    ) -> Result<(), MachineError> {
        self.machines
            .ready(machine, &admission(proven)?)
            .await
            .map_err(error)
    }
    async fn stop(&self, machine: &str, _reason: StopReason) -> Result<(), MachineError> {
        self.machines.stop(machine).await.map_err(error)
    }
    async fn remove(&self, made: &Made) -> Result<(), MachineError> {
        removable(made)?;
        self.machines.delete(made.machine()).await.map_err(error)
    }
}

/// A creation's answer. An instance that does not carry the key may be
/// someone else's, so it is never removed; any other refused one carries
/// the key, so it is the caller's.
fn made(created: &Created, request: &MachineRequest) -> Made {
    let observed = created.observed();
    let mut refusal = created.refusal();
    // What it reported, as a proof; one Agent37 named in a form no proof
    // can hold is refused as unproven, under the template asked for.
    let reported = proof(
        &observed.template,
        observed.revision,
        observed.digest.clone(),
    );
    let proven = match reported {
        Ok(proven) => proven,
        Err(_) => {
            if refusal.is_none() {
                refusal = Some(AdmissionRefusal::MissingDigest);
            }
            proof(request.template(), None, None)
                .expect("a request's template is always a proof's identity")
        }
    };
    Made::new(
        created.instance().to_owned(),
        proven,
        refusal.map(refused),
        match refusal {
            Some(AdmissionRefusal::WrongOwner) => Disposition::Retain,
            _ => Disposition::Remove,
        },
    )
}

fn proof(
    template: &str,
    revision: Option<u64>,
    digest: Option<String>,
) -> Result<Proven, MachineError> {
    let identity = match revision {
        Some(revision) => format!("{PROVEN_PREFIX}{template}#{revision}"),
        None => format!("{PROVEN_PREFIX}{template}"),
    };
    Proven::new(identity, digest)
}

/// The admission an Agent37 proof names.
///
/// # Errors
/// [`MachineError::InvalidInput`] for a proof that is not Agent37's or
/// names no digest.
fn admission(proven: &Proven) -> Result<Admission, MachineError> {
    let named = proven
        .identity()
        .strip_prefix(PROVEN_PREFIX)
        .ok_or(MachineError::InvalidInput)?;
    let (template, revision) = match named.rsplit_once('#') {
        Some((template, revision)) => (
            template,
            Some(revision.parse().map_err(|_| MachineError::InvalidInput)?),
        ),
        None => (named, None),
    };
    let digest = proven.digest().ok_or(MachineError::InvalidInput)?;
    Admission::restore(template.into(), revision, digest.into()).map_err(error)
}

fn refused(cause: AdmissionRefusal) -> Refused {
    let (cause, detail) = match cause {
        AdmissionRefusal::WrongTemplate => (RefusalCause::WrongImage, "wrong_template"),
        AdmissionRefusal::MissingDigest => (RefusalCause::Unproven, "missing_digest"),
        AdmissionRefusal::ImageChanged => (RefusalCause::Changed, "image_changed"),
        AdmissionRefusal::WrongOwner => (RefusalCause::WrongOwner, "wrong_owner"),
        AdmissionRefusal::Failed => (RefusalCause::Failed, "failed"),
    };
    Refused { cause, detail }
}

fn error(failure: Agent37Failure) -> MachineError {
    match failure {
        Agent37Failure::InvalidInput => MachineError::InvalidInput,
        Agent37Failure::Unauthorized => MachineError::Unauthorized,
        Agent37Failure::Missing => MachineError::Missing,
        Agent37Failure::Funds => MachineError::Funds,
        Agent37Failure::AdmissionRejected(cause) => MachineError::AdmissionRejected(refused(cause)),
        Agent37Failure::ProvisioningUncertain => MachineError::ProvisioningUncertain,
        Agent37Failure::Ambiguous => MachineError::Ambiguous,
        Agent37Failure::Timeout => MachineError::Timeout,
        Agent37Failure::Provider => MachineError::Provider,
    }
}

#[cfg(test)]
#[path = "cloud_tests.rs"]
mod tests;
