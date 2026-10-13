//! Credential identities and bindings shared by runtime preparation and its resolver.
//!
//! References and requirements are persistable value objects. Resolution results and
//! required-input records describe current availability. Actual material remains in
//! the separate [`super::SecretValue`] boundary.

use super::SecretValue;
use serde::{Deserialize, Serialize};

/// An application-owned credential identity, never the credential itself.
///
/// Requests and requirements may serialize this transparent string wrapper.
/// [`crate::runtime::ICredentialResolver`] maps it to a fresh [`crate::runtime::CredentialResolution`]
/// near use, allowing material to rotate without embedding it in recipes or receipts.
/// Runtime validation requires referenced identities to be nonempty.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CredentialRef(
    /// Application-owned lookup identity; this string is safe to persist as a reference.
    pub String,
);

/// The resolver's current answer for an application-owned [`crate::runtime::CredentialRef`].
///
/// This value stays in memory and has no serialization implementation. The service
/// translates unavailable material into [`crate::runtime::RequiredInput`] before installation
/// or launch, and resolves launch credentials again after setup.
#[derive(Clone, Debug)]
pub enum CredentialResolution {
    /// Resolved secret material. Preparation additionally rejects empty or NUL-bearing
    /// values as unusable input; availability alone does not test provider access.
    Available(SecretValue),
    /// The reference has no usable material in the application's credential source.
    Missing,
    /// The application knows the material has expired and must be refreshed.
    Expired,
}

/// A declarative binding from an application credential to a process variable.
///
/// Requirements contain references only. Those in [`crate::runtime::EnvironmentSpec::secrets`]
/// are supplied to the prepared workload; those in [`crate::runtime::SetupStep::secrets`] are
/// supplied only to that step's apply command, not its read-only probe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretRequirement {
    /// Environment variable receiving the resolved value. Use a valid identifier
    /// starting with an ASCII letter or underscore; reserved authentication and private
    /// directory variables are checked against the selected runtime policy.
    pub variable: String,
    /// Application-owned reference to resolve immediately before the material is used.
    pub credential: CredentialRef,
    /// Nonsecret explanation shown in [`crate::runtime::RequiredInput`] when material is
    /// unavailable, such as which registry or dataset needs access.
    pub description: String,
}

/// Why a declared credential cannot currently be supplied to the runtime.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputReason {
    /// No usable value is available. Empty or NUL-bearing resolved values are also
    /// reported as missing input rather than inserted into a process environment.
    Missing,
    /// The credential resolver explicitly reported expired material.
    Expired,
}

/// A nonsecret request for the application to supply or refresh a credential.
///
/// Returned through [`crate::runtime::PreparationOutcome::NeedsInput`] or
/// [`crate::runtime::AuthenticationState::NeedsInput`]. Update the credential source, then
/// retry the same preparation request to resume its progress.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredInput {
    /// Reference whose material must be supplied or refreshed in the resolver.
    pub credential: CredentialRef,
    /// User-facing context for the required credential; it is not secret material.
    pub description: String,
    /// Whether material is absent/unusable or explicitly known to have expired.
    pub reason: InputReason,
}
