//! The domain model of the runtime-preparation bounded context.
//!
//! This context turns explicit executable, environment, and authentication intent
//! into a checked launch capability. A preparation is a long-lived workflow
//! identified by [`PreparationRequest::id`]; [`PreparationReceipt`] is its persisted
//! progress snapshot, and [`PreparedRuntime`] is an in-memory result granted after
//! current checks. The native authentication process is an external entity;
//! [`RuntimeProcessRef`] is only its identity value.
//!
//! Recipes, requirements, commands, credential references, and authentication
//! choices are value objects compared or interpreted by their contents. Resource
//! availability, authentication state, output, and receipts describe observations
//! or progress and can become stale. Public Rust fields remain mutable; none of
//! these classifications claims that the compiler enforces immutability.
//!
//! The service and its injected interfaces live in [`crate::runtime`]. The domain
//! values do not provision compute, run OAuth, invoke models, or discover ambient
//! credentials by themselves.

#![warn(missing_docs)]

pub mod authentication;
pub mod credentials;
pub mod environment_spec;
pub mod preparation;
pub mod prepared_runtime;
pub mod resources;
pub mod runtime_command;
pub mod runtime_interaction;
pub mod runtime_kind;
pub mod runtime_process;
pub mod runtime_recipe;
pub mod secret_value;

pub use authentication::{
    Authentication, AuthenticationAction, AuthenticationActionKind, AuthenticationSession,
    AuthenticationState,
};
pub use credentials::{
    CredentialRef, CredentialResolution, InputReason, RequiredInput, SecretRequirement,
};
pub use environment_spec::EnvironmentSpec;
pub use preparation::{
    PreparationOutcome, PreparationReceipt, PreparationRequest, PreparationStage,
};
pub use prepared_runtime::PreparedRuntime;
pub use resources::{AvailableGpu, AvailableResources, GpuRequirement, ResourceRequirements};
pub use runtime_command::{RuntimeCommand, RuntimeOutput, RuntimeServerSpec};
pub use runtime_interaction::{RuntimeInputOutcome, RuntimeTerminalSize};
pub use runtime_kind::{ModelProvider, RuntimeKind};
pub use runtime_process::{RuntimeProcessRef, RuntimeProcessState};
pub use runtime_recipe::{Installation, RecipeCommand, RuntimeRecipe, SetupStep};
pub use secret_value::SecretValue;
