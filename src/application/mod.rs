//! Use cases coordinate domain operations through injected ports.
#[cfg(feature = "portability")]
mod directory_observer;
mod machines;
#[cfg(feature = "portability")]
mod recovery_service;
mod run;
#[cfg(feature = "portability")]
mod sync;
mod workspace;
mod workspace_sdk;
mod workspace_sdk_builder;

#[cfg(feature = "portability")]
pub use directory_observer::{DirectoryChanges, DirectoryObserver};
pub use machines::{MachineUse, Machines, RestReport, RestTask};
#[cfg(feature = "portability")]
pub use recovery_service::{
    RecoveryContinuationRequest, RecoveryRequest, RecoverySafety, RecoveryService,
};
pub use run::Run;
#[cfg(feature = "portability")]
pub use sync::*;
pub use workspace::{OwnedWorkspace, Workspace};
pub use workspace_sdk::WorkspaceSdk;
pub use workspace_sdk_builder::WorkspaceSdkBuilder;
