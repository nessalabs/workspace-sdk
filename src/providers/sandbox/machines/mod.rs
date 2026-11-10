//! Boat VMs as a caller's machines: made from a [`MachineSpec`], admitted
//! only in one credential posture, resumed, stopped and rolled back.
//!
//! Every Boat answer that grants anything is read strictly (`admission`): the
//! account's environment, the creation acknowledgement and the VM's full
//! info receipt. Nothing here names an application: the spec carries the
//! image, environment, lifetime and variables, and events go to the caller's
//! [`IMachineEvents`] sink, so it can keep its own audit.
//!
//! ```text
//! create:  environment ─► POST /sandboxes (key) ─► wait for start ─► check
//! ready:   check ─► wait out archiving ─► resume ─► wait ready ─► check
//! stop:    StopRequested ─► stop ─► StopAnswered
//! ```
mod admission;
mod diagnostics;
mod environment;

pub use diagnostics::{Diagnostic, Stage};
pub use environment::{
    AdmissionRefusal, AdmissionRejection, EnvironmentIdentity, ObservedEnvironment,
};

use sandbox_client::{
    wait_until_ready_with, Api, CreateSandboxEnvironmentRequest, ResumeRequest,
    SandboxInfoResponse, SandboxState, UpdateSandboxEnvironmentRequest, WaitOptions,
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::time::Instant;

/// What one VM is made from. An existing VM keeps the spec it was admitted
/// under: a caller passes its saved image and environment, not current ones.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineSpec {
    /// A snapshot id (`bsh_…`) or a named snapshot.
    pub image: String,
    /// The account environment the VM runs in, and is admitted under.
    pub environment: String,
    /// Boat's machine size, such as `small`.
    pub machine_type: String,
    /// Seconds Boat lets the VM run before archiving it; `None` is no limit.
    /// A finite value is also an upper bound the VM's receipt must honour.
    pub lifetime: Option<u32>,
    /// Environment variables set in the VM at creation.
    pub variables: BTreeMap<String, String>,
}

/// How long one operation (a creation, or making a VM ready) may wait in
/// all: for a new VM to start, an archiving VM to finish archiving, and a
/// resumed VM to be ready. Each wait spends from the same deadline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget(pub Duration);
impl Default for Budget {
    fn default() -> Self {
        Self(Duration::from_secs(120))
    }
}

/// Why a VM fails to be made or used, in the cases a caller can tell apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MachineFailure {
    /// The spec or the account's answer could not be used as given.
    InvalidInput,
    /// Boat failed, timed out, or answered something unusable.
    Provider,
    /// Boat does not know the VM or the named snapshot.
    Missing,
    /// The account's environment is absent, ambiguous, or not in the one
    /// supported posture.
    EnvironmentUnavailable,
    /// The VM, or its environment, was refused. A refused VM is never used.
    AdmissionRejected(AdmissionRejection),
    /// Boat may or may not have made a VM: ask again with the same key.
    ProvisioningUncertain,
    /// A removal was asked and not confirmed.
    CleanupUncertain,
}
impl MachineFailure {
    /// Its kind in a record: never what it carries.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid_input",
            Self::Provider => "provider",
            Self::Missing => "missing",
            Self::EnvironmentUnavailable => "environment_unavailable",
            Self::AdmissionRejected(_) => "admission_rejected",
            Self::ProvisioningUncertain => "provisioning_uncertain",
            Self::CleanupUncertain => "cleanup_uncertain",
        }
    }
}

/// Whether a refused new VM may be deleted. Only a VM this creation made
/// (Boat said `provisioning`) rolls back; a replayed or unrecognised answer
/// keeps the VM it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    RollbackNewVm,
    RetainTarget,
}
impl Disposition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RollbackNewVm => "rollback_new_vm",
            Self::RetainTarget => "retain_target",
        }
    }
}

/// What a creation returned: the VM Boat named, the environment it was
/// admitted under, and, if it was refused, why. A refused VM is still named
/// so the caller can record it and roll it back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Created {
    pub vm: String,
    pub admission: EnvironmentIdentity,
    pub refusal: Option<AdmissionRejection>,
    pub disposition: Disposition,
}

/// Why a caller stops a VM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopCause {
    /// Nothing used it for the caller's idle time.
    Idle,
    /// Its work was closed.
    Closed,
}
impl StopCause {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Closed => "closed",
        }
    }
}

/// What happened, for the caller's own records. None holds a credential,
/// a provider body or a value from a rejected receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MachineEvent {
    /// A receipt was refused at `Diagnostic`'s place.
    ReceiptRejected(Diagnostic),
    /// A VM's receipt said its inherited credentials were still being
    /// wiped; `startup_phase` is `operable`, `starting` or `other`.
    CredentialWipePending { startup_phase: &'static str },
    /// A VM is about to be stopped, its disk kept.
    StopRequested { vm: String, cause: StopCause },
    /// What Boat answered that stop; `failure` is a [`MachineFailure::kind`].
    StopAnswered {
        vm: String,
        cause: StopCause,
        failure: Option<&'static str>,
    },
    /// A rollback of a new VM was asked; `confirmed` is false unless Boat
    /// confirmed the deletion, which it never does today.
    RollbackObserved { vm: String, confirmed: bool },
}

/// The caller's sink for [`MachineEvent`]s. A failing or slow sink never
/// changes an operation's outcome, so `record` returns nothing; it must not
/// block.
pub trait IMachineEvents: Send + Sync {
    fn record(&self, event: MachineEvent);
}

/// The VMs of one Boat account.
#[derive(Clone)]
pub struct BoatMachines {
    boat: Arc<Boat>,
    budget: Budget,
}

/// One account's client, its raw-receipt HTTP client and its sink.
struct Boat {
    api: Api,
    http: reqwest::Client,
    events: Arc<dyn IMachineEvents>,
}
impl Boat {
    fn record(&self, event: MachineEvent) {
        let _ =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.events.record(event)));
    }
    /// A request to Boat's `path` with the account's credential and scope.
    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let config = self.api.config();
        let request = self
            .http
            .request(
                method,
                format!("{}/{path}", config.base_path.trim_end_matches('/')),
            )
            .bearer_auth(&config.access_token);
        match &config.org {
            Some(org) => request.header("X-Boat-Org", org),
            None => request,
        }
    }
}

impl BoatMachines {
    /// The VMs of the account `api` signs in to. `http` reads the raw
    /// creation and info receipts; it should not follow redirects or use a
    /// proxy the account did not choose.
    pub fn new(api: Api, http: reqwest::Client, events: Arc<dyn IMachineEvents>) -> Self {
        Self {
            boat: Arc::new(Boat { api, http, events }),
            budget: Budget::default(),
        }
    }
    /// The same account with another wait budget.
    pub fn with_budget(mut self, budget: Budget) -> Self {
        self.budget = budget;
        self
    }

    /// Make sure a VM could be made from `spec`: the account answers, a
    /// named image exists, and the environment exists in the one supported
    /// posture, created with it if absent.
    ///
    /// # Errors
    /// [`MachineFailure::Missing`] for an unknown named image;
    /// [`MachineFailure::EnvironmentUnavailable`] for an environment in any
    /// other posture.
    pub async fn prepare(&self, spec: &MachineSpec) -> Result<(), MachineFailure> {
        let api = &self.boat.api;
        api.me().await.map_err(|_| MachineFailure::Provider)?;
        if !spec.image.starts_with("bsh_") {
            api.get_named_snapshot(&spec.image)
                .await
                .map_err(|_| MachineFailure::Missing)?;
        }
        let mut environments = api
            .environments()
            .await
            .map_err(|_| MachineFailure::Provider)?;
        if !environments
            .environments
            .iter()
            .any(|env| env.name == spec.environment)
        {
            environments = api
                .create_environment(CreateSandboxEnvironmentRequest {
                    name: spec.environment.clone(),
                })
                .await
                .map_err(|_| MachineFailure::Provider)?;
            let created = environments
                .environments
                .iter()
                .find(|env| env.name == spec.environment)
                .ok_or(MachineFailure::Provider)?;
            api.update_environment(
                &created.id,
                UpdateSandboxEnvironmentRequest {
                    safe_for_third_parties: Some(false),
                    pass_agents_credentials: Some(true),
                    pass_github: Some(false),
                    pass_secrets: Some(false),
                    pass_sandbox_credentials: Some(false),
                    env_contents: Some(String::new()),
                    secret_files: Some(vec![]),
                    repositories: Some(vec![]),
                    credential_profile_id: Some(None),
                    proxy_ai_credentials: Some(false),
                    ..Default::default()
                },
            )
            .await
            .map_err(|_| MachineFailure::Provider)?;
        }
        admission::environment(&self.boat, spec).await?;
        Ok(())
    }

    /// Make a VM from `spec`. `key` is Boat's idempotency key: asking again
    /// with the same key answers with the same VM while Boat remembers it,
    /// so a caller saves the key before calling and asks again after an
    /// uncertain answer.
    ///
    /// A new VM is waited for until it has started, then checked in full. A
    /// refusal is returned in [`Created::refusal`], not as an error, so the
    /// VM it names is never lost.
    ///
    /// # Errors
    /// [`MachineFailure::ProvisioningUncertain`] when Boat's answer does not
    /// name a VM; the environment's own failures before any request.
    pub async fn create(&self, spec: &MachineSpec, key: &str) -> Result<Created, MachineFailure> {
        let deadline = Instant::now() + self.budget.0;
        let admission = admission::environment(&self.boat, spec).await?;
        admission::create(&self.boat, spec, key, admission, deadline).await
    }

    /// Make `vm`, admitted under `admitted` with `spec`, ready to use: wait
    /// out an archive still in progress, resume an archived VM with the
    /// spec's lifetime, wait until it is ready, and check it again.
    ///
    /// # Errors
    /// [`MachineFailure::AdmissionRejected`] when the VM or its environment
    /// changed since admission; [`MachineFailure::Provider`] when the budget
    /// runs out.
    pub async fn ready(
        &self,
        spec: &MachineSpec,
        vm: &str,
        admitted: &EnvironmentIdentity,
    ) -> Result<SandboxInfoResponse, MachineFailure> {
        let deadline = Instant::now() + self.budget.0;
        let boat = &self.boat;
        let mut found = admission::check(boat, spec, vm, admitted, None).await?;
        while found.sandbox.state == SandboxState::Archiving {
            if Instant::now() >= deadline {
                return Err(MachineFailure::Provider);
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            found = admission::check(boat, spec, vm, admitted, None).await?;
        }
        let resumed = found.sandbox.state == SandboxState::Archived;
        if resumed {
            boat.api
                .resume(
                    vm,
                    Some(ResumeRequest {
                        ttl_seconds: Some(spec.lifetime),
                        ..Default::default()
                    }),
                )
                .await
                .map_err(|_| MachineFailure::Provider)?;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        let left = u64::try_from(left.as_millis()).unwrap_or(u64::MAX);
        wait_until_ready_with(&boat.api, vm, WaitOptions::from_millis(left, 1000))
            .await
            .map_err(|_| MachineFailure::Provider)?;
        // A resumed VM takes the spec's lifetime; one already running keeps
        // the lifetime it was made or last resumed with.
        let maximum_ttl = if resumed { spec.lifetime } else { None };
        admission::check(boat, spec, vm, admitted, maximum_ttl).await
    }

    /// Stop `vm`, keeping its disk: Boat archives it, nothing runs or is
    /// billed, and [`BoatMachines::ready`] resumes it. Recorded before it is
    /// asked and once it is answered.
    ///
    /// # Errors
    /// [`MachineFailure::Provider`] when Boat refused or did not answer: the
    /// VM may still run until its lifetime ends.
    pub async fn stop(&self, vm: &str, cause: StopCause) -> Result<(), MachineFailure> {
        self.boat.record(MachineEvent::StopRequested {
            vm: vm.into(),
            cause,
        });
        let stopped = self
            .boat
            .api
            .stop(vm, None)
            .await
            .map(|_| ())
            .map_err(|_| MachineFailure::Provider);
        self.boat.record(MachineEvent::StopAnswered {
            vm: vm.into(),
            cause,
            failure: stopped.as_ref().err().map(MachineFailure::kind),
        });
        stopped
    }

    /// Delete a VM this caller's creation made and then refused, or whose
    /// record it could not save. Boat's acceptance is not confirmed erasure,
    /// so this always ends uncertain; the caller keeps its record of the VM.
    ///
    /// # Errors
    /// Always [`MachineFailure::CleanupUncertain`], or `Provider` when Boat
    /// refused.
    pub async fn roll_back(&self, vm: &str) -> Result<(), MachineFailure> {
        let removed = self.boat.api.delete_sandbox(vm).await;
        self.boat.record(MachineEvent::RollbackObserved {
            vm: vm.into(),
            confirmed: false,
        });
        removed.map_err(|_| MachineFailure::Provider)?;
        Err(MachineFailure::CleanupUncertain)
    }
}

#[cfg(test)]
#[path = "machines_tests.rs"]
mod tests;
