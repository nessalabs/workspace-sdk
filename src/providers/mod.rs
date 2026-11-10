//! Infrastructure adapters. POSIX targets require the Rust workspace-supervisor executable; the local and SSH
//! adapters execute trusted code on their hosts and do not provide a sandbox.
#[cfg(feature = "container")]
mod container;
mod i_command_runner;
mod local;
mod managed;
mod native_command;
#[cfg(feature = "boat")]
mod sandbox;
#[cfg(feature = "space")]
mod space;
mod ssh;
mod supervisor_deployment;
mod system_command_runner;
#[cfg(feature = "container")]
pub use container::{ContainerAttachOptions, ContainerConfig, ContainerProvider};
pub use i_command_runner::ICommandRunner;
pub use local::{LocalAttachOptions, LocalConfig, LocalProvider};
pub use native_command::NativeCommand;
#[cfg(feature = "boat")]
pub use sandbox::{SandboxAttachOptions, SandboxCreateOptions, SandboxProvider};
#[cfg(feature = "space")]
pub use space::{SpaceAttachOptions, SpaceConfig, SpaceProvider};
pub use ssh::{SshAttachOptions, SshConfig, SshProvider};
pub use supervisor_deployment::{SupervisorArtifact, SupervisorDeployment, TargetPlatform};
pub use system_command_runner::SystemCommandRunner;

/// Managed Boat workspaces and their creation options.
#[cfg(feature = "boat")]
pub use sandbox::{
    SandboxAttachOptions as BoatAttachOptions, SandboxCreateOptions as BoatCreateOptions,
    SandboxProvider as BoatProvider,
};
