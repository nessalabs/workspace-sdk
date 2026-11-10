//! Default persistence adapters; applications may inject their own.
mod file_state_repository;
#[cfg(feature = "portability")]
mod file_workflow_lease;
#[cfg(any(test, feature = "testing"))]
mod memory_state_repository;
#[cfg(all(any(test, feature = "testing"), feature = "portability"))]
mod memory_workflow_lease;
mod repository_state;

pub use file_state_repository::FileStateRepository;
/// In-memory state for tests. It forgets every record when dropped, so a
/// restart would create a second target for the same workspace; production
/// callers use a durable repository.
#[cfg(any(test, feature = "testing"))]
pub use memory_state_repository::MemoryStateRepository;
pub type FileWorkspaceRepository = FileStateRepository;
#[cfg(any(test, feature = "testing"))]
pub type MemoryWorkspaceRepository = MemoryStateRepository;
