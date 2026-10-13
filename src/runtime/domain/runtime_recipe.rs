//! Executable selection, installation policy, and repeatable setup specifications.
//!
//! These related value objects form a recipe rather than a live installation entity.
//! Preparation interprets them under a target-specific request and records progress
//! separately in [`super::PreparationReceipt`].

use super::{RuntimeKind, SecretRequirement};
use serde::{Deserialize, Serialize};

/// Serializable program-and-argument intent for a probe, installer, or setup step.
///
/// No string is implicitly a shell command; choosing a shell executable is an
/// explicit caller decision. Preparation binds the working directory and nonsecret
/// environment from the request and expands `{prefix}` in arguments. Resolved
/// credentials belong in [`crate::runtime::RuntimeCommand`], never this specification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeCommand {
    /// Nonempty executable name or path, without a NUL byte.
    pub program: String,
    /// Ordered arguments without NUL bytes. Each `{prefix}` occurrence is replaced
    /// with the SDK-owned installation directory before host execution.
    pub args: Vec<String>,
    /// Positive execution bound in seconds. A setup/installation command cannot
    /// select an unbounded timeout through this type.
    pub timeout_seconds: u64,
}

impl RecipeCommand {
    /// Build an ordinary program invocation with a 60-second timeout.
    ///
    /// This helper only assembles the value; request validation checks the command
    /// before execution. Adjust [`Self::timeout_seconds`] for the reviewed operation.
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            timeout_seconds: 60,
        }
    }
}

/// The allowed installation strategy when a recipe has no reusable executable.
///
/// Installation uses the request's private owned prefix and is followed by an exact
/// version probe. An existing compatible executable is reused before any installer
/// is invoked. This policy does not grant arbitrary downloaded code implicit trust.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Installation {
    /// Install the exact recipe version of a named npm package under the SDK-owned
    /// prefix. The package must be a package name (optionally scoped), not a version
    /// range, URL, shell expression, or command option.
    Npm {
        /// Registry package name without a version suffix; the recipe supplies its exact version.
        package: String,
    },
    /// Run a caller-selected installer command whose provenance is described by
    /// `source`. `{prefix}` within its arguments is replaced with the private
    /// installation directory without invoking an implicit shell.
    Command {
        /// Nonempty caller-reviewed source/provenance label included in request identity.
        /// It records intent; the SDK does not fetch or verify this label independently.
        source: String,
        /// Bounded installer invocation; the caller is responsible for making retries safe.
        command: RecipeCommand,
        /// Executable path beneath the owned prefix after installation. Must be relative
        /// and contain no `.` or `..` path components or NUL bytes.
        relative_program: String,
    },
    /// Require a compatible executable already present on the target. If absent,
    /// preparation returns a nonretryable installation failure without installing it.
    ExistingOnly,
}

/// One repeatable transition that brings a runtime environment into the desired state.
///
/// The service runs the read-only probe on every preparation, applies only after a
/// confirmed nonzero exit, then probes again. A probe transport error stops the
/// attempt without authorizing mutation. Completion is recorded only after success.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupStep {
    /// Safe identifier unique within its recipe, used in
    /// [`crate::runtime::PreparationReceipt::completed_steps`].
    pub id: String,
    /// Read-only, repeatable completion check: exit zero means the desired state is
    /// present. It receives ordinary request variables, not the step's secrets.
    pub probe: RecipeCommand,
    /// Mutation that establishes the desired state. It must be safe to repeat after
    /// interruption because the service cannot assume a failed attempt made no changes.
    pub apply: RecipeCommand,
    /// Only these declared credentials are injected into this step's apply command.
    /// Workload launch secrets and other steps' secrets are not automatically included.
    pub secrets: Vec<SecretRequirement>,
}

/// A versioned preparation specification for one executable integration.
///
/// This value object declares what to inspect, how installation may occur, and which
/// repeatable setup steps establish readiness. It is not a live runtime or a progress
/// record. [`crate::runtime::PreparationService`] checks the selected executable and setup
/// probes again when resuming a request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRecipe {
    /// Logical recipe identity used in private installation paths and receipts. It
    /// uses the same safe identifier grammar as [`crate::runtime::PreparationRequest::id`].
    pub id: String,
    /// Positive revision of this recipe definition, distinct from the tool version.
    /// Changing it changes the preparation request fingerprint.
    pub revision: u32,
    /// Integration kind that selects native authentication and private state rules.
    pub runtime: RuntimeKind,
    /// Executable to inspect on the target, as a command name or path. Npm recipes
    /// require a bare executable name for their private `node_modules/.bin` lookup.
    pub program: String,
    /// Exact semver version to install; ranges and `latest` are rejected. It must
    /// also satisfy [`Self::compatible_versions`]. Fresh installations are re-probed
    /// and must provide this exact version.
    pub version: String,
    /// Semver requirement for reusing an already available executable. A compatible
    /// existing version may differ from the exact version selected for fresh installs.
    pub compatible_versions: String,
    /// Arguments for a read-only version probe. Preparation parses a semver token
    /// from the resulting output and checks the compatible range.
    pub version_args: Vec<String>,
    /// Allowed action when no compatible executable is found. The service never
    /// infers an installer when [`crate::runtime::Installation::ExistingOnly`] is selected.
    pub installation: Installation,
    /// Ordered setup steps with unique safe identities. Each probe must establish
    /// completion before the next phase; prior receipt entries do not skip probes.
    pub setup: Vec<SetupStep>,
}
