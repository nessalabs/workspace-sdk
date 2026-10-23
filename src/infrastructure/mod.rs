//! Default persistence adapters; applications may inject their own.
mod file_state_repository;
mod file_workflow_lease;
mod local_edit_coordinator;
mod memory_state_repository;
mod memory_workflow_lease;
mod repository_state;

pub use file_state_repository::FileStateRepository;
pub use local_edit_coordinator::LocalEditCoordinator;
pub use memory_state_repository::MemoryStateRepository;
pub type FileWorkspaceRepository = FileStateRepository;
pub type MemoryWorkspaceRepository = MemoryStateRepository;
