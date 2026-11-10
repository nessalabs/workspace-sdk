//! Identity of the immutable Boat environment configuration a VM was admitted
//! under, and the bounded evidence of why one was refused.
use super::MachineFailure;

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
        let plain = !name.is_empty()
            && name.len() <= 128
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
        if !uuid(&id) || !uuid(&version_id) || !plain || version == 0 {
            return Err(MachineFailure::EnvironmentUnavailable);
        }
        Ok(Self {
            id,
            name,
            version_id,
            version,
        })
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn version_id(&self) -> &str {
        &self.version_id
    }
    pub fn version(&self) -> u32 {
        self.version
    }
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
    pub name: Option<String>,
    pub version: Option<u32>,
}
/// Why a VM, or the account's environment, was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionRefusal {
    MissingEnvironment,
    WrongEnvironment,
    MissingVersion,
    WrongVersion,
    EnvironmentChanged,
    EnvironmentUnavailable,
    InvalidReceipt,
    ReceiptUnavailable,
    LifetimeNotEnforced,
}
impl AdmissionRefusal {
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
    pub fn cause(&self) -> AdmissionRefusal {
        self.cause
    }
    pub fn observed(&self) -> Option<&ObservedEnvironment> {
        self.observed.as_ref()
    }
}

#[cfg(test)]
#[path = "environment_tests.rs"]
mod tests;
