//! Intent, progress snapshots, and outcomes for the runtime-preparation workflow.
//!
//! The preparation identity stays stable across retries. The service owns transitions
//! and verifies the request fingerprint; changing a returned Rust snapshot does not
//! mutate persisted progress or grant launch authority.

use super::{
    Authentication, AuthenticationAction, EnvironmentSpec, PreparedRuntime, RequiredInput,
    RuntimeProcessRef, RuntimeRecipe,
};
use serde::{Deserialize, Serialize};

/// The complete, serializable intent for preparing one runtime on one target.
///
/// [`crate::runtime::PreparationService`] fingerprints the entire request and associates it
/// with [`Self::id`]. Reuse the identical request to resume; changing a recipe, path,
/// environment, or authentication choice requires a new preparation identity.
/// Credential references may resolve to rotated material without changing the request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparationRequest {
    /// Stable preparation identity. Must be 1–128 ASCII letters, digits, dots, dashes,
    /// or underscores, excluding `.` and `..`. Reusing it with a different request
    /// returns a conflict.
    pub id: String,
    /// Nonempty host/workspace identity, included in the request fingerprint and
    /// receipt so intent is tied to its target. The injected host must address that target.
    pub target_id: String,
    /// Private SDK-owned absolute directory for installations and native auth state.
    /// It must be outside the working directory, normalized without `.`/`..`, and
    /// excluded from exported or synchronized project files. The host enforces privacy.
    pub owned_root: String,
    /// Normalized, non-root absolute target directory in which probes, setup, native
    /// authentication, and prepared workload commands run.
    pub working_directory: String,
    /// Versioned executable, installation policy, and ordered setup steps to prepare.
    pub recipe: RuntimeRecipe,
    /// Resource constraints, ordinary environment variables, and workload secrets.
    pub environment: EnvironmentSpec,
    /// Explicit runtime authentication/billing choice; validation rejects unsupported
    /// runtime/provider combinations rather than silently selecting another method.
    pub authentication: Authentication,
}

/// The latest phase recorded while satisfying a [`crate::runtime::PreparationRequest`].
///
/// A resumed preparation rechecks current resources, executable, setup probes, and
/// authentication. Stages record progress; they are not a cache that skips validation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreparationStage {
    /// Initial request, resource, and credential validation; no installation has yet
    /// been recorded for a newly created receipt.
    Validating,
    /// Inspecting reusable software or running the selected bounded installer.
    Installing,
    /// An executable was selected and setup steps are being probed/applied/verified.
    Configuring,
    /// Setup is complete and the explicitly selected credential or native login
    /// method is being established or checked.
    Authenticating,
    /// The service produced a current [`crate::runtime::PreparedRuntime`]. The receipt alone
    /// cannot recreate that value or prove that credentials remain valid later.
    Ready,
}

/// Serializable progress and identity for resumable runtime preparation.
///
/// Stored through [`crate::runtime::IPreparationStore`]. It contains identifiers, selected
/// executable metadata, and progress only: no resolved credentials, native command
/// output, or environment values. A receipt cannot be deserialized into a
/// [`crate::runtime::PreparedRuntime`]; preparation must recheck the target.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparationReceipt {
    /// Identity copied from [`crate::runtime::PreparationRequest::id`], used as the store key.
    pub id: String,
    /// Host/workspace identity copied from the request for correlation and audit.
    pub target_id: String,
    /// Exact host namespace that performed preparation. Unscoped legacy receipts
    /// must be explicitly reconciled instead of being rebound to the current host.
    pub host_scope: String,
    /// SHA-256 hex digest of the serialized complete request, used to detect reuse of
    /// a preparation identity for different intent. It is not a hash of secret material.
    pub request_fingerprint: String,
    /// Logical recipe identity selected by the request.
    pub recipe_id: String,
    /// Recipe definition revision, separate from the requested executable version.
    pub recipe_revision: u32,
    /// Latest recorded phase; a resumed attempt can revisit earlier phases.
    pub stage: PreparationStage,
    /// Selected executable after successful version probing. May be a compatible
    /// existing program or an SDK-owned installation; absent before selection.
    pub installed_program: Option<String>,
    /// Semver observed for the selected executable, absent before successful probing.
    pub installed_version: Option<String>,
    /// Setup step identities that previously passed their probes. Every step is still
    /// rechecked on resume, so these entries do not authorize skipping a probe.
    pub completed_steps: Vec<String>,
    /// Native login process to reconnect to or inspect before starting another
    /// attempt. A stored reference does not mean the process is still running.
    pub authentication_process: Option<RuntimeProcessRef>,
    /// Zero-based native login attempt counter used in the host idempotency key.
    /// Incremented only after the previous known process has exited.
    pub authentication_attempt: u32,
    /// Whether the explicit one-time Codex cache provisioning action completed. Once
    /// true, preparation uses target-owned native auth state rather than copying again.
    pub codex_cache_provisioned: bool,
}

/// The application-facing result of advancing runtime preparation.
///
/// Expected pauses and setup failures retain a nonsecret progress snapshot. Transport,
/// validation, unsupported-operation, and persistence errors may instead be returned
/// as [`crate::Error`] by the service. Only [`Self::Ready`] grants a fresh launch plan.
#[derive(Clone, Debug)]
pub enum PreparationOutcome {
    /// The current target passed preparation and authentication checks. The contained
    /// capability can build commands with freshly resolved credential material.
    Ready(PreparedRuntime),
    /// Preparation needs caller-managed credential material before it can continue.
    NeedsInput {
        /// Nonsecret progress snapshot to retain while the application handles this outcome.
        receipt: PreparationReceipt,
        /// Credential references and reasons to present to the application for resolution.
        inputs: Vec<RequiredInput>,
    },
    /// Installation and setup reached native authentication, which needs user action.
    NeedsAuthentication {
        /// Nonsecret progress snapshot to retain while the application handles this outcome.
        receipt: PreparationReceipt,
        /// Current native interaction to pass to the preparation service after user action.
        action: AuthenticationAction,
    },
    /// An installer, setup operation, or native API-key setup did not establish the
    /// required state. The snapshot identifies the phase and retry policy.
    SetupFailed {
        /// Nonsecret progress snapshot to retain while the application handles this outcome.
        receipt: PreparationReceipt,
        /// Phase in which preparation failed to establish the required state.
        stage: PreparationStage,
        /// Bounded nonsecret explanation; it excludes raw installer or native auth output.
        message: String,
        /// Whether retrying the same intent is supported after addressing the failure;
        /// this is not a guarantee that an immediate retry will succeed.
        retryable: bool,
    },
}
