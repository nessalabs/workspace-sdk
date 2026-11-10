//! What a caller asks for, how long it may wait, and how it can fail. Each
//! value is checked when it is made, so nothing reaches Boat that Boat would
//! only refuse after it had made (and billed) a VM.
use super::environment::AdmissionRejection;
use std::{collections::BTreeMap, fmt, time::Duration};

/// The most variables one VM is made with.
pub const MAX_VARIABLES: usize = 64;
/// The longest variable value, in bytes.
pub const MAX_VARIABLE_VALUE: usize = 4096;
/// The longest [`Budget`] a caller may give.
pub const MAX_BUDGET: Duration = Duration::from_secs(24 * 60 * 60);

/// What one VM is made from. An existing VM keeps the spec it was admitted
/// under: a caller passes its saved image and environment, not current ones.
///
/// Its `Debug` shows variable names only, never their values.
#[derive(Clone, PartialEq, Eq)]
pub struct MachineSpec {
    image: String,
    environment: String,
    machine_type: String,
    lifetime: Option<u32>,
    variables: BTreeMap<String, String>,
}

impl MachineSpec {
    /// A spec for VMs made from `image` (a snapshot id `bsh_…` or a named
    /// snapshot), in the account environment `environment`, of Boat's size
    /// `machine_type` (such as `small`). `lifetime` is the seconds Boat lets
    /// a VM run before archiving it, `None` for no limit; a finite value is
    /// also an upper bound the VM's receipt must honour. `variables` are set
    /// in the VM when it is made.
    ///
    /// # Errors
    /// [`MachineFailure::InvalidInput`] unless the image, environment and
    /// type are each one plain segment (letters, digits, `-`, `_`, `.`) of
    /// at most 128 bytes, `lifetime` is above zero, and there are at most
    /// [`MAX_VARIABLES`] variables, each named like a shell variable (at most
    /// 128 bytes) with a value of at most [`MAX_VARIABLE_VALUE`] bytes and
    /// no NUL.
    pub fn new(
        image: impl Into<String>,
        environment: impl Into<String>,
        machine_type: impl Into<String>,
        lifetime: Option<u32>,
        variables: BTreeMap<String, String>,
    ) -> Result<Self, MachineFailure> {
        let spec = Self {
            image: image.into(),
            environment: environment.into(),
            machine_type: machine_type.into(),
            lifetime,
            variables,
        };
        let named = |name: &str| {
            !name.is_empty()
                && name.len() <= 128
                && !name.as_bytes()[0].is_ascii_digit()
                && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        };
        if !plain_segment(&spec.image)
            || !plain_segment(&spec.environment)
            || !plain_segment(&spec.machine_type)
            || spec.lifetime == Some(0)
            || spec.variables.len() > MAX_VARIABLES
            || spec.variables.iter().any(|(name, value)| {
                !named(name) || value.len() > MAX_VARIABLE_VALUE || value.contains('\0')
            })
        {
            return Err(MachineFailure::InvalidInput);
        }
        Ok(spec)
    }
    /// The snapshot a VM is made from.
    pub fn image(&self) -> &str {
        &self.image
    }
    /// The account environment a VM runs in, and is admitted under.
    pub fn environment(&self) -> &str {
        &self.environment
    }
    /// Boat's machine size.
    pub fn machine_type(&self) -> &str {
        &self.machine_type
    }
    /// Seconds a VM runs before Boat archives it; `None` is no limit.
    pub fn lifetime(&self) -> Option<u32> {
        self.lifetime
    }
    /// Variables set in a VM when it is made.
    pub fn variables(&self) -> &BTreeMap<String, String> {
        &self.variables
    }
}

impl fmt::Debug for MachineSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MachineSpec")
            .field("image", &self.image)
            .field("environment", &self.environment)
            .field("machine_type", &self.machine_type)
            .field("lifetime", &self.lifetime)
            .field("variables", &self.variables.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// One name Boat puts in a path or a query: letters, digits, `-`, `_` and
/// `.`, from 1 to 128 bytes.
pub(super) fn plain_segment(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// How long one whole operation may take: [`BoatMachines::create`] or
/// [`BoatMachines::ready`](super::BoatMachines::ready), every request and
/// wait in it included. Each step spends from one deadline set when the
/// operation starts.
///
/// [`BoatMachines::create`]: super::BoatMachines::create
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget(Duration);
impl Budget {
    /// # Errors
    /// [`MachineFailure::InvalidInput`] for zero, or for more than
    /// [`MAX_BUDGET`].
    pub fn new(duration: Duration) -> Result<Self, MachineFailure> {
        if duration.is_zero() || duration > MAX_BUDGET {
            return Err(MachineFailure::InvalidInput);
        }
        Ok(Self(duration))
    }
    /// How long it lasts.
    pub fn duration(self) -> Duration {
        self.0
    }
}
impl Default for Budget {
    /// Two minutes.
    fn default() -> Self {
        Self(Duration::from_secs(120))
    }
}

/// Why a VM fails to be made or used, in the cases a caller can tell apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MachineFailure {
    /// The caller's spec, key or VM id could not be used as given; nothing
    /// was asked of Boat.
    InvalidInput,
    /// Boat failed, refused, timed out, or answered something unusable, or
    /// the operation's [`Budget`] ran out.
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

#[cfg(test)]
#[path = "spec_tests.rs"]
mod tests;
