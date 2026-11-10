//! Ports implemented by infrastructure and injected into application services.
mod i_backend;
mod i_connection;
mod i_provider;
#[cfg(feature = "portability")]
mod i_recovery_repository;
#[cfg(feature = "portability")]
mod i_workflow_lease;
#[cfg(feature = "portability")]
mod i_workflow_repository;
mod i_workspace_repository;

pub use i_backend::IBackend;
pub use i_connection::{
    ConnectionInput, ConnectionOutput, ConnectionStream, FileChannel, IConnection,
    IConnectionProcess, ProcessEnd, ProcessExit, RunOutput, SocketForward, MAX_RUN_OUTPUT,
    MAX_SOCKET_PATH, MAX_STDERR,
};
pub use i_provider::IProvider;
#[cfg(feature = "portability")]
pub use i_recovery_repository::IRecoveryRepository;
#[cfg(feature = "portability")]
pub use i_workflow_lease::IWorkflowLease;
#[cfg(feature = "portability")]
pub use i_workflow_repository::IWorkflowRepository;
pub use i_workspace_repository::IWorkspaceRepository;
