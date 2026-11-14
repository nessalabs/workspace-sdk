//! One Agent37 workspace's instances as a caller's machines: made under a
//! key, admitted, woken, stopped and deleted.
//!
//! ```text
//! create:  find by key ─► (none) POST /instances ─► admit
//!                       └► (lost answer) find by key again
//! ready:   GET ─► admit ─► start if stopped or asleep ─► wait for running
//! stop:    GET ─► stop unless already stopped or asleep
//! ```
//!
//! Agent37 takes no idempotency key, so a creation tags its instance with
//! the caller's key and looks for that tag before and after asking. Two
//! creations with one key at the same moment can still make two instances:
//! callers serialize creations per key, as the SDK's records do, and
//! [`Agent37Machines::create`] refuses to choose between two it finds.
use super::{
    api::{Api, Instance, NewInstance, Refusal, Status},
    config::Agent37Config,
    failure::{AdmissionRefusal, Agent37Failure},
    identity::Agent37Identity,
    spec::MachineSpec,
};
use std::{collections::BTreeMap, time::Duration};
use tokio::time::Instant;

/// The metadata name an instance's creation key is kept under.
pub(super) const KEY_TAG: &str = "workspace_sdk_key";
/// How often a wait reads the instance again.
const POLL: Duration = Duration::from_secs(2);
/// How long making a machine ready may take by default.
const READY_BUDGET: Duration = Duration::from_secs(180);

/// What an instance was admitted under: the template release it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Admission {
    template: String,
    revision: Option<u64>,
    digest: String,
}
impl Admission {
    /// An admission the caller saved and loads again.
    ///
    /// # Errors
    /// [`Agent37Failure::InvalidInput`] for an empty template or digest.
    pub fn restore(
        template: String,
        revision: Option<u64>,
        digest: String,
    ) -> Result<Self, Agent37Failure> {
        if template.is_empty() || digest.is_empty() {
            return Err(Agent37Failure::InvalidInput);
        }
        Ok(Self {
            template,
            revision,
            digest,
        })
    }
    /// The pinned template, as the instance names it.
    pub fn template(&self) -> &str {
        &self.template
    }
    /// The template's revision, for a workspace template.
    pub fn revision(&self) -> Option<u64> {
        self.revision
    }
    /// The image digest the instance runs.
    pub fn digest(&self) -> &str {
        &self.digest
    }
}

/// What a creation returned: the instance, what it was admitted under,
/// and whether this creation made it or found it under the key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Created {
    instance: String,
    admission: Admission,
    made: bool,
}
impl Created {
    /// The instance's id.
    pub fn instance(&self) -> &str {
        &self.instance
    }
    /// What it was admitted under; pass it to [`Agent37Machines::ready`].
    pub fn admission(&self) -> &Admission {
        &self.admission
    }
    /// Whether this call made it, rather than finding one an earlier
    /// attempt with the same key made.
    pub fn made(&self) -> bool {
        self.made
    }
}

/// The instances of one Agent37 workspace.
///
/// `stop` and `delete` act for the whole workspace: they do not check that
/// this process made the instance. Hand an `Agent37Machines` only to code
/// that owns the workspace.
#[derive(Clone, Debug)]
pub struct Agent37Machines {
    api: Api,
    budget: Duration,
}
impl Agent37Machines {
    /// The instances of the workspace `config` reaches.
    ///
    /// # Errors
    /// [`Agent37Failure::Provider`] when no HTTP client can be built.
    pub fn new(config: Agent37Config) -> Result<Self, Agent37Failure> {
        Ok(Self {
            api: Api::new(config)?,
            budget: READY_BUDGET,
        })
    }
    /// The same workspace with another budget for each readiness.
    pub fn with_budget(mut self, budget: Duration) -> Self {
        self.budget = budget;
        self
    }

    /// Make sure machines could be made from `spec`: the key works and the
    /// pinned template release exists.
    ///
    /// # Errors
    /// [`Agent37Failure::Unauthorized`] for a refused key;
    /// [`Agent37Failure::Missing`] for a template or release that is not
    /// published.
    pub async fn prepare(&self, spec: &MachineSpec) -> Result<(), Agent37Failure> {
        self.api.template(spec.template()).await?;
        Ok(())
    }

    /// Register `identity`'s public key for every instance of the
    /// workspace. Registering it again is accepted.
    ///
    /// # Errors
    /// [`Agent37Failure::InvalidInput`] when the workspace holds its 20
    /// keys already, or refuses this one.
    pub async fn register(
        &self,
        identity: &Agent37Identity,
        name: &str,
    ) -> Result<(), Agent37Failure> {
        Ok(self.api.register_key(identity.public_key(), name).await?)
    }

    /// The instance made from `spec` under `key`: the one an earlier
    /// attempt with `key` made, or a new one. `key` must be the same on
    /// every attempt for one machine and never reused for another.
    ///
    /// # Errors
    /// [`Agent37Failure::ProvisioningUncertain`] when an instance may have
    /// been made but was not found: ask again with the same key.
    /// [`Agent37Failure::Ambiguous`] when two instances carry the key.
    /// [`Agent37Failure::AdmissionRejected`] when the instance found or
    /// made is not what `spec` asks for; it is left for the caller, who may
    /// delete it. [`Agent37Failure::Funds`] when the wallet or instance
    /// limit refuses.
    pub async fn create(&self, spec: &MachineSpec, key: &str) -> Result<Created, Agent37Failure> {
        valid_key(key)?;
        if let Some(found) = self.find(key).await? {
            return admit(&found, spec, key).map(|admission| Created {
                instance: found.id,
                admission,
                made: false,
            });
        }
        let name = format!(
            "{}-{}",
            spec.template_name(),
            &key[key.len().saturating_sub(12)..]
        );
        let new = NewInstance {
            template: spec.template(),
            name: &name,
            region: &spec.region,
            resources: spec.size,
            metadata: BTreeMap::from([(KEY_TAG, key)]),
            env: &spec.variables,
            auto_sleep: true,
            idle_timeout_seconds: spec.idle.as_secs(),
        };
        let made = match self.api.create(&new).await {
            Ok(made) => made,
            // No answer: it may have been made, and may still be starting.
            Err(Refusal::Unknown) => match self.find(key).await {
                Ok(Some(found)) => found,
                _ => return Err(Agent37Failure::ProvisioningUncertain),
            },
            Err(refusal) => return Err(refusal.into()),
        };
        if made.tag(KEY_TAG) != Some(key) {
            return Err(Agent37Failure::AdmissionRejected(
                AdmissionRefusal::WrongOwner,
            ));
        }
        let admission = admit(&made, spec, key)?;
        Ok(Created {
            instance: made.id,
            admission,
            made: true,
        })
    }

    /// The live instance tagged `key`, if there is one.
    async fn find(&self, key: &str) -> Result<Option<Instance>, Agent37Failure> {
        let mut tagged = self
            .api
            .instances()
            .await?
            .into_iter()
            .filter(|instance| instance.tag(KEY_TAG) == Some(key))
            .filter(|instance| !matches!(instance.status, Status::Deleting | Status::Deleted));
        let first = tagged.next();
        if tagged.next().is_some() {
            return Err(Agent37Failure::Ambiguous);
        }
        Ok(first)
    }

    /// Make `instance` ready to use: check it still runs what it was
    /// admitted under, start it if it is stopped or asleep, and wait until
    /// it runs, within the budget.
    ///
    /// # Errors
    /// [`Agent37Failure::AdmissionRejected`] when it no longer runs the
    /// admitted image; [`Agent37Failure::Timeout`] past the budget;
    /// [`Agent37Failure::Missing`] for an instance that is gone.
    pub async fn ready(&self, instance: &str, admitted: &Admission) -> Result<(), Agent37Failure> {
        valid_instance(instance)?;
        let deadline = Instant::now() + self.budget;
        let mut started = false;
        loop {
            // A read that fails is tried again until the deadline: it
            // changes nothing.
            if let Some(found) = self.read(instance).await? {
                if found.template != admitted.template
                    || found.image_digest.as_deref() != Some(admitted.digest.as_str())
                {
                    return Err(Agent37Failure::AdmissionRejected(
                        AdmissionRefusal::ImageChanged,
                    ));
                }
                match found.status {
                    Status::Running => return Ok(()),
                    status if status.gone() => {
                        return Err(Agent37Failure::AdmissionRejected(AdmissionRefusal::Failed))
                    }
                    status if status.at_rest() && !started => {
                        match self.api.lifecycle(instance, "start").await {
                            Ok(()) => started = true,
                            Err(refusal) if refusal.transient() => {}
                            Err(refusal) => return Err(refusal.into()),
                        }
                    }
                    // Starting, waking, stopping, or a start still finishing.
                    _ => {}
                }
            }
            if Instant::now() + POLL > deadline {
                return Err(Agent37Failure::Timeout);
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// `instance` as the API reads it now; `None` when the read had no
    /// answer, and an error only for one the API refused.
    async fn read(&self, instance: &str) -> Result<Option<Instance>, Agent37Failure> {
        match self.api.instance(instance).await {
            Ok(found) => Ok(Some(found)),
            Err(Refusal::Unknown) => Ok(None),
            Err(refusal) if refusal.transient() => Ok(None),
            Err(refusal) => Err(refusal.into()),
        }
    }

    /// Stop `instance`, keeping its disk. One already stopped or asleep is
    /// left as it is: either bills its disk alone.
    ///
    /// # Errors
    /// [`Agent37Failure::Timeout`] when it could not be stopped within the
    /// budget; it may still be running.
    pub async fn stop(&self, instance: &str) -> Result<(), Agent37Failure> {
        valid_instance(instance)?;
        let deadline = Instant::now() + self.budget;
        loop {
            if let Some(found) = self.read(instance).await? {
                match found.status {
                    status if status.at_rest() || status.gone() => return Ok(()),
                    Status::Running => match self.api.lifecycle(instance, "stop").await {
                        Ok(()) => return Ok(()),
                        Err(refusal) if refusal.transient() => {}
                        Err(Refusal::Unknown) => {}
                        Err(refusal) => return Err(refusal.into()),
                    },
                    _ => {}
                }
            }
            if Instant::now() + POLL > deadline {
                return Err(Agent37Failure::Timeout);
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// Delete `instance` and everything on it. One already gone is
    /// accepted: the API answers the same for an instance that is not this
    /// workspace's.
    ///
    /// # Errors
    /// [`Agent37Failure::Provider`] when the deletion's outcome is unknown.
    pub async fn delete(&self, instance: &str) -> Result<(), Agent37Failure> {
        valid_instance(instance)?;
        match self.api.delete(instance).await {
            Ok(()) => Ok(()),
            Err(refusal) if refusal.status() == Some(404) => Ok(()),
            Err(refusal) => Err(refusal.into()),
        }
    }
}

/// What `instance`, tagged `key`, was admitted under, if it is what `spec`
/// asks for.
fn admit(instance: &Instance, spec: &MachineSpec, key: &str) -> Result<Admission, Agent37Failure> {
    let refused = |cause| Err(Agent37Failure::AdmissionRejected(cause));
    if instance.tag(KEY_TAG) != Some(key) {
        return refused(AdmissionRefusal::WrongOwner);
    }
    if instance.status.gone() {
        return refused(AdmissionRefusal::Failed);
    }
    // One still being made may not name its image yet: it is judged once
    // it has finished, by asking again with the same key.
    if matches!(instance.status, Status::Provisioning | Status::Unknown) {
        return Err(Agent37Failure::ProvisioningUncertain);
    }
    if instance.template != spec.template() {
        return refused(AdmissionRefusal::WrongTemplate);
    }
    let Some(digest) = instance
        .image_digest
        .clone()
        .filter(|digest| !digest.is_empty())
    else {
        return refused(AdmissionRefusal::MissingDigest);
    };
    Ok(Admission {
        template: instance.template.clone(),
        revision: instance.template_revision,
        digest,
    })
}

/// An instance id: 10 lowercase alphanumerics, which is also a DNS label.
pub(super) fn valid_instance(id: &str) -> Result<(), Agent37Failure> {
    if id.len() == 10
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        Ok(())
    } else {
        Err(Agent37Failure::InvalidInput)
    }
}

fn valid_key(key: &str) -> Result<(), Agent37Failure> {
    if (8..=128).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        Ok(())
    } else {
        Err(Agent37Failure::InvalidInput)
    }
}

#[cfg(test)]
#[path = "machines_tests.rs"]
mod tests;
