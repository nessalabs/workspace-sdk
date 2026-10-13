//! Authentication choices, required interactions, and observations within runtime preparation.
//!
//! The choice is explicit request intent. Actions describe work that remains; states
//! and sessions describe what the service last observed. Native CLIs retain ownership
//! of their login processes and token lifecycle.

use super::{CredentialRef, RequiredInput, RuntimeProcessRef};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The caller's explicit authentication and billing choice for a runtime.
///
/// This is serializable configuration: variants name [`crate::runtime::CredentialRef`] values,
/// never secret material. [`crate::runtime::PreparationService`] checks that the choice is
/// supported by the selected [`crate::runtime::RuntimeKind`]. Native subscription state and
/// API credentials use separate private directories, so resuming a preparation does
/// not silently switch its billing method.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum Authentication {
    /// No model authentication is required. Valid only for [`crate::runtime::RuntimeKind::Generic`];
    /// workload-specific secrets may still be declared in [`crate::runtime::EnvironmentSpec`].
    None,
    /// Use the target CLI's native end-user subscription login. Supported for Codex,
    /// unmodified Claude Code, and OpenCode with OpenAI. Missing native state produces
    /// an [`crate::runtime::AuthenticationAction`]; this never falls back to API billing.
    Subscription,
    /// Use an explicitly resolved API key. The runtime/provider chooses the expected
    /// environment variable; Codex also registers it through native private-stdin login.
    /// Generic and custom-provider runtimes must use [`Self::ProviderCredentials`].
    ApiKey {
        /// Application-owned reference resolved when preparing or checking authentication.
        /// The referenced value is supplied through a private credential channel.
        credential: CredentialRef,
    },
    /// Explicitly provision caller-supplied, file-backed Codex `auth.json` once into
    /// the target's private directory. Supported only for Codex. The SDK neither discovers
    /// a local auth store nor continuously mirrors it; a separate
    /// [`crate::runtime::AuthenticationActionKind::ProvisionCodexCache`] action performs the write.
    CodexAuthCache {
        /// Reference resolving to the JSON-object contents of caller-selected `auth.json`.
        credential: CredentialRef,
    },
    /// Supply named inference-provider credentials, such as a third-party cloud
    /// provider's environment variables. This selects explicit provider credentials
    /// rather than a native subscription-token portability mechanism.
    ProviderCredentials {
        /// Nonempty mapping of environment variable names to application credential
        /// references. Names must pass runtime validation and cannot override reserved
        /// private-directory variables or inject `CLAUDE_CODE_OAUTH_TOKEN`.
        variables: BTreeMap<String, CredentialRef>,
    },
}

/// The user interaction needed to establish the selected native authentication.
///
/// The service advertises an action only after inspecting the installed CLI and its
/// current login state. Pass it back through [`crate::runtime::AuthenticationAction`] to
/// [`crate::runtime::PreparationService::start_authentication`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthenticationActionKind {
    /// Start the installed CLI's supported device or headless browser login. The CLI
    /// owns the codes, browser interaction, tokens, and refresh behavior.
    DeviceLogin,
    /// Run the unmodified CLI's native login in a target terminal. A host without
    /// terminal support may return [`crate::Error::Unsupported`]; callers with their
    /// own terminal can obtain [`crate::runtime::PreparationService::login_command`].
    NativeTerminalRequired,
    /// Write the explicitly resolved Codex cache to the private target auth directory
    /// once, then ask the native CLI whether it recognizes a subscription session.
    ProvisionCodexCache,
}

/// A nonsecret, resumable request for authentication interaction.
///
/// It is tied to one preparation and checked against the current required action
/// before execution. A stale action cannot change the selected authentication method.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticationAction {
    /// Identity of the [`crate::runtime::PreparationRequest`] that requires this action.
    pub preparation_id: String,
    /// Interaction to perform; the service verifies that this kind is still current.
    pub kind: AuthenticationActionKind,
    /// Human-readable next step, suitable for an application prompt. It contains no
    /// resolved credential or native command output.
    pub description: String,
}

/// The observed result of starting or resuming native authentication.
///
/// A session may describe a still-running login, already recognized credentials, or
/// missing input. Starting a process is not proof of successful login; inspect
/// [`Self::state`] and re-run preparation after the user completes the interaction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticationSession {
    /// Preparation identity to use when reconnecting to this authentication flow.
    pub preparation_id: String,
    /// Native login process, when one was started or reconnected. Cache provisioning
    /// can complete without a process, and a stored process may already have exited.
    pub process: Option<RuntimeProcessRef>,
    /// Authentication observation after starting or resuming the action. A native
    /// status check, not the presence of a process reference, determines readiness.
    pub state: AuthenticationState,
}

/// A point-in-time authentication observation for the explicitly selected method.
///
/// These states distinguish locally available material from a native subscription
/// login and from required user action. They do not establish upstream entitlement
/// or guarantee that a later model request will succeed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthenticationState {
    /// The generic runtime selected [`crate::runtime::Authentication::None`].
    NotRequired,
    /// All credentials for the selected API or inference-provider method resolve to
    /// nonempty, NUL-free values. No billable model call was made to validate them.
    CredentialsAvailable,
    /// The native CLI locally recognizes a subscription session of the selected kind.
    /// Upstream access may subsequently expire or be revoked.
    NativeSessionPresent,
    /// One or more caller-managed credential references are missing, expired, or
    /// unusable. Supply the listed inputs through the credential resolver and retry.
    NeedsInput(Vec<RequiredInput>),
    /// The native CLI does not recognize the selected subscription login. The action
    /// describes the supported interaction that can establish it.
    NeedsAuthentication(AuthenticationAction),
}
