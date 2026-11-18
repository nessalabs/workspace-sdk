//! A caller's machines on any cloud, in one set of values: what it asks
//! for, what a machine was proven to run, how a cloud stops a machine left
//! running, and how an operation fails. The port is
//! [`crate::ports::IMachines`]; each cloud's adapter maps its own types onto
//! these.
//!
//! ```text
//! MachineRequest ─► create(key) ─► Made { machine, proven, refusal?, disposition }
//!                                      │ saved by the caller
//!                   ready(machine, proven) ◄┘   (proven compared exactly)
//!                   stop(machine, reason)       remove(made) if Remove
//! ```
use std::{collections::BTreeMap, fmt, time::Duration};

/// The most variables one machine is made with.
pub const MAX_MACHINE_VARIABLES: usize = 64;
/// The longest variable value, in bytes.
pub const MAX_MACHINE_VARIABLE: usize = 4096;
/// The longest [`Proven`] identity or digest, in bytes.
pub const MAX_PROVEN: usize = 512;

/// What one machine is made from, on whichever cloud.
///
/// Its `Debug` shows variable names only, never their values.
#[derive(Clone, PartialEq, Eq)]
pub struct MachineRequest {
    template: String,
    lifetime: Lifetime,
    variables: BTreeMap<String, String>,
}
impl MachineRequest {
    /// A machine made from `template` (the cloud's image or pinned
    /// template), stopped by its cloud as `lifetime` says, holding
    /// `variables` in its environment. Each cloud checks the template's
    /// form, and whether it can keep the lifetime ([`Backstop`]).
    ///
    /// # Errors
    /// [`MachineError::InvalidInput`] for an empty template or one over 128
    /// bytes or with anything but printable ASCII, more than
    /// [`MAX_MACHINE_VARIABLES`] variables, a variable not named like a
    /// shell variable (at most 128 bytes), or a value over
    /// [`MAX_MACHINE_VARIABLE`] bytes or holding NUL.
    pub fn new(
        template: impl Into<String>,
        lifetime: Lifetime,
        variables: BTreeMap<String, String>,
    ) -> Result<Self, MachineError> {
        let template = template.into();
        let named = |name: &str| {
            !name.is_empty()
                && name.len() <= 128
                && !name.as_bytes()[0].is_ascii_digit()
                && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        };
        if template.is_empty()
            || template.len() > 128
            || !template.bytes().all(|b| b.is_ascii_graphic())
            || lifetime.duration().is_some_and(|d| d.is_zero())
            || variables.len() > MAX_MACHINE_VARIABLES
            || variables.iter().any(|(name, value)| {
                !named(name) || value.len() > MAX_MACHINE_VARIABLE || value.contains('\0')
            })
        {
            return Err(MachineError::InvalidInput);
        }
        Ok(Self {
            template,
            lifetime,
            variables,
        })
    }
    /// The image or pinned template.
    pub fn template(&self) -> &str {
        &self.template
    }
    /// How its cloud stops it when nothing else does.
    pub fn lifetime(&self) -> Lifetime {
        self.lifetime
    }
    /// Variables set in it when it is made.
    pub fn variables(&self) -> &BTreeMap<String, String> {
        &self.variables
    }
}
impl fmt::Debug for MachineRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MachineRequest")
            .field("template", &self.template)
            .field("lifetime", &self.lifetime)
            .field("variables", &self.variables.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// How a machine's cloud stops it if its caller never does: the backstop
/// against one left running and billing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifetime {
    /// Never: only its caller stops it.
    Unlimited,
    /// After it has run this long, whatever moves through it.
    Hard(Duration),
    /// After this long with nothing moving through it. An open connection's
    /// keepalives count as movement.
    Idle(Duration),
}
impl Lifetime {
    /// The duration it names, if any.
    pub fn duration(self) -> Option<Duration> {
        match self {
            Self::Unlimited => None,
            Self::Hard(duration) | Self::Idle(duration) => Some(duration),
        }
    }
}

/// Which [`Lifetime`]s a cloud enforces itself. An application decides
/// which it requires before choosing a cloud.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backstop {
    /// A hard lifetime, which the cloud reports back and is checked.
    HardLifetime,
    /// Idle sleep, within these bounds; never a hard lifetime.
    IdleSleep {
        /// The shortest idle time it takes.
        shortest: Duration,
        /// The longest.
        longest: Duration,
    },
}
impl Backstop {
    /// Whether a machine of `lifetime` can be made on this cloud.
    /// [`Lifetime::Unlimited`] always can.
    pub fn enforces(self, lifetime: Lifetime) -> bool {
        match (self, lifetime) {
            (_, Lifetime::Unlimited) | (Self::HardLifetime, Lifetime::Hard(_)) => true,
            (Self::IdleSleep { shortest, longest }, Lifetime::Idle(idle)) => {
                (shortest..=longest).contains(&idle)
            }
            _ => false,
        }
    }
}

/// What a machine was proven to run when it was admitted: its cloud's own
/// identity for it and, where the cloud reports one, an immutable digest.
/// A later readiness compares both exactly, so a machine that changed what
/// it runs is refused.
///
/// The identity is opaque to callers: save it and hand it back.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Proven {
    identity: String,
    digest: Option<String>,
}
impl Proven {
    /// A proof as its cloud's adapter makes it, or as a caller loads it
    /// again.
    ///
    /// # Errors
    /// [`MachineError::InvalidInput`] unless the identity, and the digest
    /// when given, are each 1 to [`MAX_PROVEN`] printable ASCII bytes.
    pub fn new(identity: String, digest: Option<String>) -> Result<Self, MachineError> {
        let plain = |value: &str| {
            !value.is_empty()
                && value.len() <= MAX_PROVEN
                && value.bytes().all(|b| b.is_ascii_graphic())
        };
        if !plain(&identity) || digest.as_deref().is_some_and(|digest| !plain(digest)) {
            return Err(MachineError::InvalidInput);
        }
        Ok(Self { identity, digest })
    }
    /// The cloud's identity for what it runs.
    pub fn identity(&self) -> &str {
        &self.identity
    }
    /// The immutable digest of what it runs, if its cloud reports one.
    pub fn digest(&self) -> Option<&str> {
        self.digest.as_deref()
    }
}

/// Whether a machine a creation answered may be removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// It is the caller's alone: made under its key. It may be removed.
    Remove,
    /// It may be in use elsewhere, or not be the caller's: never removed.
    Retain,
}

/// Why a machine is refused. A refused machine is never used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefusalCause {
    /// It does not carry the caller's key: it may be someone else's.
    WrongOwner,
    /// It runs another image, template, environment or version than asked.
    WrongImage,
    /// What it runs changed since it was admitted.
    Changed,
    /// Its cloud did not prove what it runs: a missing digest or an
    /// unreadable or late receipt.
    Unproven,
    /// Its cloud does not enforce the lifetime asked for.
    LifetimeNotEnforced,
    /// Its cloud still has to clean it before it may be used.
    Pending,
    /// It failed, or is being deleted.
    Failed,
}

/// A refusal: its cause, and the cloud's own name for it, for records and
/// people (never matched on).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    /// Why.
    pub cause: RefusalCause,
    /// The cloud's own name for why, such as `wrong_environment`.
    pub detail: &'static str,
}

/// What a creation answered: the machine, what it was proven to run,
/// whether it was refused, and whether it may be removed. A refused machine
/// is still named, so the caller can record it and remove it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Made {
    machine: String,
    proven: Proven,
    refusal: Option<Refused>,
    disposition: Disposition,
}
impl Made {
    /// A creation's answer, as an adapter makes it.
    pub fn new(
        machine: String,
        proven: Proven,
        refusal: Option<Refused>,
        disposition: Disposition,
    ) -> Self {
        Self {
            machine,
            proven,
            refusal,
            disposition,
        }
    }
    /// A creation the caller saved and loads again, so it can still remove
    /// a machine it made. It holds no refusal: the caller kept whatever it
    /// decided.
    pub fn restore(machine: String, proven: Proven, disposition: Disposition) -> Self {
        Self::new(machine, proven, None, disposition)
    }
    /// The machine's id on its cloud.
    pub fn machine(&self) -> &str {
        &self.machine
    }
    /// What it was proven to run; for a refused machine, what it reported
    /// or what was asked for. Pass it to [`crate::ports::IMachines::ready`]
    /// only when it was not refused.
    pub fn proven(&self) -> &Proven {
        &self.proven
    }
    /// Why it was refused, if it was.
    pub fn refusal(&self) -> Option<&Refused> {
        self.refusal.as_ref()
    }
    /// Whether it may be removed.
    pub fn disposition(&self) -> Disposition {
        self.disposition
    }
}

/// Why a machine is being stopped, for its cloud's records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// Nothing used it for the caller's idle time.
    Idle,
    /// Its work was closed.
    Closed,
    /// It was woken to be checked or used and could not be.
    NotReady,
    /// It was refused, or its check did not finish.
    NotAdmitted,
}

/// Why a machine operation did not succeed, on whichever cloud. It never
/// holds the cloud's own words.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MachineError {
    /// The request, key or machine id could not be used as given; nothing
    /// was asked of the cloud.
    #[error("invalid input")]
    InvalidInput,
    /// The cloud does not know the machine, image or template.
    #[error("missing")]
    Missing,
    /// The cloud refused the caller's credential.
    #[error("unauthorized")]
    Unauthorized,
    /// The account's balance or limits do not allow it.
    #[error("the account's balance or limits do not allow it")]
    Funds,
    /// The account's environment is absent or not in a supported posture.
    #[error("environment unavailable")]
    EnvironmentUnavailable,
    /// The machine was refused.
    #[error("not admitted: {}", .0.detail)]
    AdmissionRejected(Refused),
    /// A machine may or may not have been made: ask again with the same key.
    #[error("provisioning uncertain")]
    ProvisioningUncertain,
    /// More than one machine carries the key. Asking again does not settle
    /// it: the owner removes the extra one.
    #[error("ambiguous: more than one machine carries the key")]
    Ambiguous,
    /// The machine did not become ready, or stop, within the budget.
    #[error("timed out")]
    Timeout,
    /// A removal was asked and not confirmed.
    #[error("cleanup uncertain")]
    CleanupUncertain,
    /// Anything else the cloud or the network did.
    #[error("the cloud failed")]
    Provider,
}

#[cfg(test)]
#[path = "machines_tests.rs"]
mod tests;
