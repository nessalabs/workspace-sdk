//! Infrastructure adapters. POSIX targets require the Rust workspace-supervisor executable; the local and SSH
//! adapters execute trusted code on their hosts and do not provide a sandbox.
mod container;
mod i_command_runner;
mod local;
mod managed;
mod native_command;
#[cfg(feature = "boat")]
mod sandbox;
mod ssh;
mod supervisor_deployment;
mod system_command_runner;
pub use container::{ContainerConfig, ContainerOpenOptions, ContainerProvider};
pub use i_command_runner::ICommandRunner;
pub use local::{LocalConfig, LocalOpenOptions, LocalProvider};
pub use native_command::NativeCommand;
#[cfg(feature = "boat")]
pub use sandbox::{SandboxOpenOptions, SandboxProvider};
pub use ssh::{SshConfig, SshOpenOptions, SshProvider};
pub use supervisor_deployment::{SupervisorArtifact, SupervisorDeployment, TargetPlatform};
pub use system_command_runner::SystemCommandRunner;

/// Boat's public names are aliases over the neutral sandbox adapter.
#[cfg(feature = "boat")]
pub use sandbox::{SandboxOpenOptions as BoatOpenOptions, SandboxProvider as BoatProvider};
