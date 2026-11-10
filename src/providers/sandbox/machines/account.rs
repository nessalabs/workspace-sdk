//! One Boat account's VMs: made, made ready, stopped and rolled back.
use super::{
    admission,
    environment::{AdmissionRefusal, AdmissionRejection, EnvironmentIdentity},
    events::{IMachineEvents, MachineEvent, StopCause},
    spec::{Budget, MachineFailure, MachineSpec},
};
use sandbox_client::{
    wait_until_ready_with, Api, CreateSandboxEnvironmentRequest, ResumeRequest,
    SandboxInfoResponse, SandboxState, UpdateSandboxEnvironmentRequest, WaitOptions,
};
use std::{sync::Arc, time::Duration};
use tokio::time::Instant;

/// The only billing scope the admission checks accept: a VM in any other
/// scope has a team and shared access, so its receipt is always refused.
const PERSONAL: &str = "personal";

/// Whether a refused new VM may be deleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// Boat said this creation made the VM (`provisioning`), so the caller
    /// may delete it with [`BoatMachines::roll_back`].
    RollbackNewVm,
    /// Boat replayed an older VM (`ready`) or answered in a way not
    /// recognised: the VM may be in use elsewhere and is never deleted.
    RetainTarget,
}
impl Disposition {
    /// Its name in a record.
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
///
/// Admitted means its receipt passed, not that it runs: a replayed VM may be
/// archived. [`BoatMachines::ready`] makes it ready before use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Created {
    pub(super) vm: String,
    pub(super) admission: EnvironmentIdentity,
    pub(super) refusal: Option<AdmissionRejection>,
    pub(super) disposition: Disposition,
}
impl Created {
    /// A creation the caller saved and loads again, such as after a restart,
    /// so it can still roll back a VM it made. It holds no refusal: the
    /// caller kept whatever it decided.
    ///
    /// # Errors
    /// [`MachineFailure::InvalidInput`] for a malformed `vm`.
    pub fn restore(
        vm: String,
        admission: EnvironmentIdentity,
        disposition: Disposition,
    ) -> Result<Self, MachineFailure> {
        if !admission::vm_id(&vm) {
            return Err(MachineFailure::InvalidInput);
        }
        Ok(Self {
            vm,
            admission,
            refusal: None,
            disposition,
        })
    }
    /// The VM Boat named.
    pub fn vm(&self) -> &str {
        &self.vm
    }
    /// The environment it was admitted under; pass it to
    /// [`BoatMachines::ready`].
    pub fn admission(&self) -> &EnvironmentIdentity {
        &self.admission
    }
    /// Why it was refused, if it was. A refused VM is never used.
    pub fn refusal(&self) -> Option<&AdmissionRejection> {
        self.refusal.as_ref()
    }
    /// Whether it may be rolled back.
    pub fn disposition(&self) -> Disposition {
        self.disposition
    }
}

/// The VMs of one Boat account, in its personal scope.
#[derive(Clone)]
pub struct BoatMachines {
    boat: Arc<Boat>,
    budget: Budget,
}

/// One account's client, its raw-receipt HTTP client and its sink.
pub(super) struct Boat {
    pub(super) api: Api,
    http: reqwest::Client,
    events: Arc<dyn IMachineEvents>,
}
impl Boat {
    pub(super) fn record(&self, event: MachineEvent) {
        let _ =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.events.record(event)));
    }
    /// A request to Boat's `path` with the account's credential, in the
    /// personal scope.
    pub(super) fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let config = self.api.config();
        self.http
            .request(
                method,
                format!("{}/{path}", config.base_path.trim_end_matches('/')),
            )
            .bearer_auth(&config.access_token)
            .header("X-Boat-Org", PERSONAL)
    }
}

impl BoatMachines {
    /// The VMs of the account `api` signs in to. Every call is made in the
    /// account's personal scope, whatever scope `api` was given: VMs in a
    /// team's scope are never admitted.
    ///
    /// `http` reads the raw creation and info receipts, each cut at 64 KiB.
    /// The caller gives it connect and request timeouts (a few seconds to
    /// connect, under a minute per request is typical), no redirects and no
    /// proxy the account did not choose; `api` keeps its own. Those
    /// timeouts are what bound [`BoatMachines::prepare`] and
    /// [`BoatMachines::stop`]; creation and readiness are also bounded by
    /// the [`Budget`].
    pub fn new(api: Api, http: reqwest::Client, events: Arc<dyn IMachineEvents>) -> Self {
        Self {
            boat: Arc::new(Boat {
                api: api.with_org(Some(PERSONAL.into())),
                http,
                events,
            }),
            budget: Budget::default(),
        }
    }
    /// The same account with another budget for each creation and each
    /// readiness.
    pub fn with_budget(mut self, budget: Budget) -> Self {
        self.budget = budget;
        self
    }

    /// The account's client, in its personal scope.
    pub(in crate::providers::sandbox) fn api(&self) -> &Api {
        &self.boat.api
    }

    fn deadline(&self) -> Result<Instant, MachineFailure> {
        Instant::now()
            .checked_add(self.budget.duration())
            .ok_or(MachineFailure::InvalidInput)
    }

    /// Make sure a VM could be made from `spec`: the account answers, a
    /// named image exists, and the environment exists in the one supported
    /// posture, created with it if absent.
    ///
    /// An environment Boat made whose posture could then not be set stays
    /// in Boat's default posture, and this keeps failing
    /// `EnvironmentUnavailable` until someone fixes or deletes it; this never
    /// changes an environment it did not just make.
    ///
    /// # Errors
    /// [`MachineFailure::Missing`] for an unknown named image;
    /// [`MachineFailure::EnvironmentUnavailable`] for an environment in any
    /// other posture; [`MachineFailure::Provider`] when Boat fails.
    pub async fn prepare(&self, spec: &MachineSpec) -> Result<(), MachineFailure> {
        let api = &self.boat.api;
        api.me().await.map_err(|_| MachineFailure::Provider)?;
        if !spec.image().starts_with("bsh_") {
            api.get_named_snapshot(spec.image())
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
            .any(|env| env.name == spec.environment())
        {
            environments = api
                .create_environment(CreateSandboxEnvironmentRequest {
                    name: spec.environment().into(),
                })
                .await
                .map_err(|_| MachineFailure::Provider)?;
            let created = environments
                .environments
                .iter()
                .find(|env| env.name == spec.environment())
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
    /// refusal, or a check the budget cut short, is returned in
    /// [`Created::refusal`], not as an error, so the VM it names is never
    /// lost.
    ///
    /// # Errors
    /// [`MachineFailure::InvalidInput`] unless `key` is 1 to 255 visible
    /// ASCII bytes; [`MachineFailure::ProvisioningUncertain`] for any answer
    /// that does not name a VM, a refusal included, since an earlier attempt
    /// with the same key may have made one, or when the budget ran out after
    /// the request; the environment's own failures before any request.
    pub async fn create(&self, spec: &MachineSpec, key: &str) -> Result<Created, MachineFailure> {
        if key.is_empty() || key.len() > 255 || !key.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(MachineFailure::InvalidInput);
        }
        let deadline = self.deadline()?;
        let admission = tokio::time::timeout_at(deadline, admission::environment(&self.boat, spec))
            .await
            .map_err(|_| MachineFailure::Provider)??;
        admission::create(&self.boat, spec, key, admission, deadline).await
    }

    /// Make `vm`, admitted under `admitted` with `spec`, ready to use: wait
    /// out an archive still in progress, resume an archived VM with the
    /// spec's lifetime, wait until it is ready, and check it again.
    ///
    /// # Errors
    /// [`MachineFailure::InvalidInput`] for a malformed `vm`;
    /// [`MachineFailure::AdmissionRejected`] when the VM or its environment
    /// changed since admission; [`MachineFailure::Provider`] when Boat fails,
    /// the VM does not become ready, or the budget runs out. A VM this call
    /// resumed is stopped again on any failure ([`StopCause::NotReady`]); one
    /// it found running is left as it was. Another caller that starts using
    /// the VM between its resume and that stop loses it.
    pub async fn ready(
        &self,
        spec: &MachineSpec,
        vm: &str,
        admitted: &EnvironmentIdentity,
    ) -> Result<SandboxInfoResponse, MachineFailure> {
        if !admission::vm_id(vm) {
            return Err(MachineFailure::InvalidInput);
        }
        let deadline = self.deadline()?;
        let mut resumed = false;
        let ready = tokio::time::timeout_at(
            deadline,
            self.ready_by(spec, vm, admitted, deadline, &mut resumed),
        )
        .await
        .map_err(|_| MachineFailure::Provider)
        .and_then(|ready| ready);
        if ready.is_err() && resumed {
            // Only a VM this call woke is put back; one found running may be
            // in use elsewhere.
            let _ = self.stop(vm, StopCause::NotReady).await;
        }
        ready
    }

    /// The check before waking `vm`. A wipe still pending on a stopped VM
    /// ends with its restart, which resuming is, so it lets a stopped VM
    /// through; the full check after it is ready decides.
    async fn before_waking(
        &self,
        spec: &MachineSpec,
        vm: &str,
        admitted: &EnvironmentIdentity,
    ) -> Result<SandboxInfoResponse, MachineFailure> {
        match admission::check(&self.boat, spec, vm, admitted, None).await {
            Err(MachineFailure::AdmissionRejected(rejection))
                if rejection.cause() == AdmissionRefusal::WipePending =>
            {
                let found = self
                    .boat
                    .api
                    .get(vm)
                    .await
                    .map_err(|_| MachineFailure::Provider)?;
                if matches!(
                    found.sandbox.state,
                    SandboxState::Archived | SandboxState::Archiving
                ) {
                    Ok(found)
                } else {
                    Err(MachineFailure::AdmissionRejected(rejection))
                }
            }
            checked => checked,
        }
    }

    async fn ready_by(
        &self,
        spec: &MachineSpec,
        vm: &str,
        admitted: &EnvironmentIdentity,
        deadline: Instant,
        resumed_it: &mut bool,
    ) -> Result<SandboxInfoResponse, MachineFailure> {
        let boat = &self.boat;
        let mut found = self.before_waking(spec, vm, admitted).await?;
        while found.sandbox.state == SandboxState::Archiving {
            tokio::time::sleep(Duration::from_secs(1)).await;
            found = self.before_waking(spec, vm, admitted).await?;
        }
        let resumed = found.sandbox.state == SandboxState::Archived;
        if resumed {
            *resumed_it = true;
            boat.api
                .resume(
                    vm,
                    Some(ResumeRequest {
                        ttl_seconds: Some(spec.lifetime()),
                        ..Default::default()
                    }),
                )
                .await
                .map_err(|_| MachineFailure::Provider)?;
        }
        // The client reads a zero wait as no deadline at all, so a spent
        // budget ends here instead.
        let left = deadline
            .saturating_duration_since(Instant::now())
            .as_millis();
        let left = u64::try_from(left).unwrap_or(u64::MAX);
        if left == 0 {
            return Err(MachineFailure::Provider);
        }
        wait_until_ready_with(&boat.api, vm, WaitOptions::from_millis(left, 1000))
            .await
            .map_err(|_| MachineFailure::Provider)?;
        // A resumed VM takes the spec's lifetime; one already running keeps
        // the lifetime it was made or last resumed with.
        let maximum_ttl = if resumed { spec.lifetime() } else { None };
        let found = admission::check(boat, spec, vm, admitted, maximum_ttl).await?;
        if !found.sandbox.state.is_operable() {
            return Err(MachineFailure::Provider);
        }
        Ok(found)
    }

    /// Stop `vm`, keeping its disk: Boat archives it, nothing runs or is
    /// billed, and [`BoatMachines::ready`] resumes it. Recorded before it is
    /// asked and once it is answered.
    ///
    /// # Errors
    /// [`MachineFailure::InvalidInput`] for a malformed `vm`, with nothing
    /// asked or recorded; [`MachineFailure::Provider`] when Boat refused or
    /// did not answer: the VM may still run until its lifetime ends.
    pub async fn stop(&self, vm: &str, cause: StopCause) -> Result<(), MachineFailure> {
        if !admission::vm_id(vm) {
            return Err(MachineFailure::InvalidInput);
        }
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

    /// Delete a VM this caller's creation made: one it refused, or one whose
    /// record the caller could not save. Boat's acceptance is not confirmed
    /// erasure, so this always ends uncertain; the caller keeps its record
    /// of the VM.
    ///
    /// # Errors
    /// [`MachineFailure::InvalidInput`], with nothing asked, unless
    /// `created` is [`Disposition::RollbackNewVm`]: a VM Boat replayed may
    /// be in use elsewhere. Otherwise always
    /// [`MachineFailure::CleanupUncertain`], or `Provider` when Boat refused.
    pub async fn roll_back(&self, created: &Created) -> Result<(), MachineFailure> {
        if created.disposition != Disposition::RollbackNewVm {
            return Err(MachineFailure::InvalidInput);
        }
        let removed = self.boat.api.delete_sandbox(&created.vm).await;
        self.boat.record(MachineEvent::RollbackObserved {
            vm: created.vm.clone(),
            confirmed: false,
        });
        removed.map_err(|_| MachineFailure::Provider)?;
        Err(MachineFailure::CleanupUncertain)
    }
}

#[cfg(test)]
#[path = "machines_tests.rs"]
mod tests;
