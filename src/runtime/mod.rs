//! Runtime preparation is the bounded context that turns explicit tool and
//! authentication intent into a checked workload launch plan.
//!
//! Start with a [`RuntimeRecipe`] (or a constructor such as [`codex`] or [`generic`]),
//! an [`EnvironmentSpec`], and an explicit [`Authentication`] choice in a
//! [`PreparationRequest`]. [`PreparationService::prepare`] validates the request,
//! checks host resources and required credentials, reuses or installs compatible
//! software, verifies repeatable setup steps, and checks the selected authentication.
//!
//! A [`PreparationOutcome`] distinguishes a fresh [`PreparedRuntime`] from missing
//! inputs, a native authentication action, or a setup failure. Retain the
//! [`PreparationReceipt`] to reconnect with the same intent, but run preparation
//! again to obtain a launch capability: receipts are progress snapshots and never
//! replace checks of the current target or freshly resolved credentials.
//!
//! [`IRuntimeHost`], [`ICredentialResolver`], and [`IPreparationStore`] are injected
//! interfaces separating execution, credential lookup, and persistence from the
//! preparation policy. [`BackendRuntimeHost`] adapts a workspace execution backend.
//! Callers own compute provisioning and workload launch; the preparation context
//! does not implement OAuth, select billing implicitly, or issue model requests.
//!
//! Native CLIs own subscription sign-in. Their private state lives outside the
//! synchronized project and is scoped by runtime, model provider, authentication
//! method, and credential references. [`SecretValue`] material stays in memory and
//! private execution channels; serializable recipes and receipts contain references
//! or progress rather than resolved values. Workload output is a separate boundary
//! and can still contain material that the workload itself prints.
//!
//! [`domain`] groups related concepts and explains their roles as value objects,
//! external identities, observations, and workflow snapshots. These roles do not
//! imply compiler-enforced immutability for types with public Rust fields.

pub mod domain;
pub(crate) mod infrastructure;

mod authentication_policy;
mod backend_host;
mod command_factory;
mod credential_policy;
mod file_preparation_store;
mod i_credential_resolver;
mod i_preparation_store;
mod i_runtime_host;
mod memory_preparation_store;
mod no_credentials;
mod preparation_service;
mod preparation_validation;
mod recipes;
mod runtime_paths;

pub use backend_host::BackendRuntimeHost;
pub use file_preparation_store::FilePreparationStore;
pub use i_credential_resolver::ICredentialResolver;
pub use i_preparation_store::{IPreparationLease, IPreparationStore};
pub use i_runtime_host::IRuntimeHost;
pub use memory_preparation_store::MemoryPreparationStore;
pub use no_credentials::NoCredentials;
pub use preparation_service::PreparationService;
pub use recipes::*;

pub use domain::*;
