//! Identity of the immutable Boat environment configuration a VM was admitted
//! under, and the bounded evidence of why one was refused.
use super::spec::{plain_segment, MachineFailure};

/// The environment a VM was admitted under: its id, name, and the exact
/// version. A VM is used only while the account's environment is still this
/// one and the VM's own receipt names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvironmentIdentity {
    id: String,
    name: String,
    version_id: String,
    version: u32,
}
impl EnvironmentIdentity {
    /// The environment `id` named `name`, at version `version_id`, number
    /// `version`.
    ///
    /// # Errors
    /// [`MachineFailure::EnvironmentUnavailable`] unless both ids are
    /// canonical non-nil UUIDs, the name is one plain segment and the
    /// version is above zero.
    pub fn new(
        id: String,
        name: String,
        version_id: String,
        version: u32,
    ) -> Result<Self, MachineFailure> {
        let uuid =
            |s: &str| uuid::Uuid::parse_str(s).is_ok_and(|id| id.to_string() == s && !id.is_nil());
        if !uuid(&id) || !uuid(&version_id) || !plain_segment(&name) || version == 0 {
            return Err(MachineFailure::EnvironmentUnavailable);
        }
        Ok(Self {
            id,
            name,
            version_id,
            version,
        })
    }
    /// The environment's id.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// The environment's name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// The id of the version admitted.
    pub fn version_id(&self) -> &str {
        &self.version_id
    }
    /// The number of the version admitted.
    pub fn version(&self) -> u32 {
        self.version
    }
    /// Whether a VM's receipt names this environment, at this version.
    ///
    /// # Errors
    /// The [`AdmissionRefusal`] for the first thing that differs.
    pub fn check(&self, observed: &ObservedEnvironment) -> Result<(), AdmissionRefusal> {
        match (&observed.name, observed.version) {
            (None, _) => Err(AdmissionRefusal::MissingEnvironment),
            (Some(name), _) if name != &self.name => Err(AdmissionRefusal::WrongEnvironment),
            (_, None) => Err(AdmissionRefusal::MissingVersion),
            (_, Some(version)) if version != self.version => Err(AdmissionRefusal::WrongVersion),
            _ => Ok(()),
        }
    }
}

/// What a VM's receipt says about its environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedEnvironment {
    /// The environment name it gives, if any.
    pub name: Option<String>,
    /// The environment version number it gives, if any.
    pub version: Option<u32>,
}
/// Why a VM, or the account's environment, was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionRefusal {
    /// The VM's receipt names no environment.
    MissingEnvironment,
    /// It names another environment.
    WrongEnvironment,
    /// It names no environment version.
    MissingVersion,
    /// It names another version.
    WrongVersion,
    /// The account's environment is no longer the one admitted.
    EnvironmentChanged,
    /// The account's environment is gone or not in the supported posture.
    EnvironmentUnavailable,
    /// A receipt could not be read strictly, or grants too much.
    InvalidReceipt,
    /// No usable receipt arrived in time.
    ReceiptUnavailable,
    /// The VM's lifetime is missing or longer than the spec allows.
    LifetimeNotEnforced,
    /// Boat still has to wipe credentials the VM's image carried; it may
    /// hold another account's logins until it restarts.
    WipePending,
}
impl AdmissionRefusal {
    /// Its name in a record.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingEnvironment => "missing_environment",
            Self::WrongEnvironment => "wrong_environment",
            Self::MissingVersion => "missing_version",
            Self::WrongVersion => "wrong_version",
            Self::EnvironmentChanged => "environment_changed",
            Self::EnvironmentUnavailable => "environment_unavailable",
            Self::InvalidReceipt => "invalid_receipt",
            Self::ReceiptUnavailable => "receipt_unavailable",
            Self::LifetimeNotEnforced => "lifetime_not_enforced",
            Self::WipePending => "wipe_pending",
        }
    }
    /// What it means, for people.
    pub fn describe(self) -> &'static str {
        match self {
            Self::MissingEnvironment => "the machine's receipt names no environment",
            Self::WrongEnvironment => "the machine runs in another environment",
            Self::MissingVersion => "the machine's receipt names no environment version",
            Self::WrongVersion => "the machine runs another version of its environment",
            Self::EnvironmentChanged => {
                "the account's environment changed after the machine was admitted"
            }
            Self::EnvironmentUnavailable => {
                "the account's environment is gone or passes more than agent credentials"
            }
            Self::InvalidReceipt => {
                "the machine's receipt could not be read strictly or grants too much"
            }
            Self::ReceiptUnavailable => "no usable receipt for the machine arrived in time",
            Self::LifetimeNotEnforced => {
                "the machine's lifetime is missing or longer than the one asked for"
            }
            Self::WipePending => "the machine still has credentials from its image to wipe",
        }
    }
}

/// Immutable observed configuration facts; admission owns their one supported posture.
pub(super) struct EnvironmentConfiguration {
    pub(super) safe_for_third_parties: bool,
    pub(super) pass_github: bool,
    pub(super) pass_secrets: bool,
    pub(super) pass_sandbox_credentials: bool,
    pub(super) pass_agents_credentials: bool,
    pub(super) has_contents: bool,
    pub(super) has_secret_files: bool,
    pub(super) has_repositories: bool,
    pub(super) owner_default_profile: bool,
    pub(super) proxy_ai_credentials: bool,
}
impl EnvironmentConfiguration {
    pub(super) fn admit(self) -> Result<(), MachineFailure> {
        if self.safe_for_third_parties
            || self.pass_github
            || self.pass_secrets
            || self.pass_sandbox_credentials
            || !self.pass_agents_credentials
            || self.has_contents
            || self.has_secret_files
            || self.has_repositories
            || !self.owner_default_profile
            || self.proxy_ai_credentials
        {
            return Err(MachineFailure::EnvironmentUnavailable);
        }
        Ok(())
    }
}

/// Bounded refusal evidence; provider-controlled bodies and credentials never enter it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmissionRejection {
    cause: AdmissionRefusal,
    observed: Option<ObservedEnvironment>,
}
impl AdmissionRejection {
    /// A refusal for `cause`, with what the receipt said. A name too long,
    /// not ASCII or holding control characters is not kept: the refusal
    /// becomes [`AdmissionRefusal::InvalidReceipt`] with nothing observed.
    pub fn new(cause: AdmissionRefusal, observed: Option<ObservedEnvironment>) -> Self {
        if observed
            .as_ref()
            .and_then(|o| o.name.as_ref())
            .is_some_and(|n| n.len() > 128 || !n.is_ascii() || n.chars().any(char::is_control))
        {
            return Self {
                cause: AdmissionRefusal::InvalidReceipt,
                observed: None,
            };
        }
        Self { cause, observed }
    }
    /// Why it was refused.
    pub fn cause(&self) -> AdmissionRefusal {
        self.cause
    }
    /// What the receipt said, when it could be kept.
    pub fn observed(&self) -> Option<&ObservedEnvironment> {
        self.observed.as_ref()
    }
}
impl std::fmt::Display for AdmissionRejection {
    /// The cause in words, then what the receipt said: the name is kept only
    /// when it is short plain ASCII, so it is safe to show.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.cause.describe())?;
        if let Some(observed) = &self.observed {
            match (&observed.name, observed.version) {
                (Some(name), Some(version)) => write!(f, " (it names {name:?} version {version})"),
                (Some(name), None) => write!(f, " (it names {name:?})"),
                (None, Some(version)) => write!(f, " (it names version {version})"),
                (None, None) => Ok(()),
            }?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "environment_tests.rs"]
mod tests;
