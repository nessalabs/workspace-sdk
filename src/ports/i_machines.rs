//! A caller's machines on one cloud: made under a key, admitted, made
//! ready, stopped and removed. Every cloud's adapter answers in the same
//! [`crate::domain::machines`] values, so an application holds one
//! `Arc<dyn IMachines>` per configured cloud and picks between them.
use crate::domain::machines::{Backstop, MachineError, MachineRequest, Made, Proven, StopReason};
use async_trait::async_trait;

/// One account's machines on one cloud.
///
/// `stop` and `remove` act for the whole account: they do not check that
/// this process made the machine. Hand one only to code that owns the
/// account.
#[async_trait]
pub trait IMachines: Send + Sync {
    /// Which lifetimes this cloud enforces itself.
    fn backstop(&self) -> Backstop;

    /// Make sure machines could be made from `request`: the account answers
    /// and the template exists. Changes nothing a caller could see, except
    /// where a cloud must make an environment once.
    ///
    /// # Errors
    /// [`MachineError::Missing`] for an unknown template;
    /// [`MachineError::Unauthorized`] for a refused credential;
    /// [`MachineError::InvalidInput`] for a lifetime [`Self::backstop`]
    /// does not enforce.
    async fn prepare(&self, request: &MachineRequest) -> Result<(), MachineError>;

    /// The machine made from `request` under `key`: one an earlier attempt
    /// with `key` made, or a new one. `key` is the same on every attempt
    /// for one machine and never reused for another; callers serialize
    /// attempts per key. A machine found or made that is not what `request`
    /// asks for is answered with [`Made::refusal`] set, not as an error.
    ///
    /// # Errors
    /// [`MachineError::ProvisioningUncertain`] when a machine may have been
    /// made: ask again with the same key. [`MachineError::Ambiguous`] when
    /// more than one carries the key.
    async fn create(&self, request: &MachineRequest, key: &str) -> Result<Made, MachineError>;

    /// Make `machine` ready to use: check it still runs what `proven` says,
    /// wake it if it rests, and wait until it runs. `request` is the one it
    /// was made from.
    ///
    /// # Errors
    /// [`MachineError::AdmissionRejected`] when it no longer runs what was
    /// proven; [`MachineError::Timeout`] or [`MachineError::Provider`] when
    /// it does not become ready in time; [`MachineError::Missing`] for a
    /// machine that is gone.
    async fn ready(
        &self,
        request: &MachineRequest,
        machine: &str,
        proven: &Proven,
    ) -> Result<(), MachineError>;

    /// Stop `machine`, keeping its disk.
    ///
    /// # Errors
    /// [`MachineError::Provider`] or [`MachineError::Timeout`] when it may
    /// still run.
    async fn stop(&self, machine: &str, reason: StopReason) -> Result<(), MachineError>;

    /// Remove a machine a creation answered, and everything on it.
    ///
    /// # Errors
    /// [`MachineError::InvalidInput`], with nothing asked, unless `made` is
    /// [`Disposition::Remove`](crate::domain::machines::Disposition::Remove) ([`Made::removable`]). [`MachineError::CleanupUncertain`] when the
    /// cloud accepted the removal but does not confirm it.
    async fn remove(&self, made: &Made) -> Result<(), MachineError>;
}

/// How a cloud lets a retried creation find what an earlier attempt made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Idempotency {
    /// Its create takes the key itself and answers a retry with the same
    /// target.
    Native,
    /// Its create takes no key: the target is tagged with it, and found by
    /// that tag.
    Tagged,
}

/// What a creation request answered.
#[derive(Debug)]
pub enum Submitted<T> {
    /// It named the target.
    Named(T),
    /// It may or may not have made one: the answer was lost, or refused
    /// in a way that does not rule out an earlier attempt's.
    Unknown,
}

/// One cloud's creation of one kind of target, for
/// [`crate::providers::find_or_create`].
#[async_trait]
pub trait ICreationSite: Send + Sync {
    /// What a creation makes, as the cloud reports it.
    type Target: Send;
    /// How the cloud fails.
    type Error: Send;
    /// How a retry finds an earlier attempt's target.
    fn idempotency(&self) -> Idempotency;
    /// Every live target tagged `key`. Asked only of a
    /// [`Idempotency::Tagged`] cloud, which must answer it; by default none.
    async fn find(&self, _key: &str) -> Result<Vec<Self::Target>, Self::Error> {
        Ok(Vec::new())
    }
    /// Ask the cloud to make a target under `key`: tagged with it, or
    /// with it as the cloud's own idempotency key.
    async fn submit(&self, key: &str) -> Result<Submitted<Self::Target>, Self::Error>;
}
