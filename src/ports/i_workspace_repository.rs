use crate::{domain::*, Result};

/// Persistence interface for each workspace's one machine record.
///
/// Applications can implement this with files, a database, or memory. The role
/// is workspace persistence, not file storage; [`crate::infrastructure::FileStateRepository`]
/// is one implementation rather than a requirement of the domain. Every change
/// goes through [`MachineRecord::advance`], so a store cannot accept a state
/// move the domain forbids.
pub trait IWorkspaceRepository: Send + Sync {
    /// Reserve `id` for `request` before any side effect, saving `initial`.
    ///
    /// If the ID already holds a record for the same request, return it
    /// unchanged; if it holds any other record, return `Conflict`.
    fn begin(
        &self,
        id: &WorkspaceId,
        request: &str,
        initial: MachineState,
    ) -> Result<MachineRecord>;
    /// Replace the record at `expected_revision` with the legal move to `next`.
    /// A different saved revision returns `Conflict`.
    fn advance(
        &self,
        id: &WorkspaceId,
        expected_revision: u64,
        next: MachineState,
    ) -> Result<MachineRecord>;
    /// Delete a record that is still Provisioning or Attaching at
    /// `expected_revision`. Ready and Refused records are never deleted here.
    fn forget(&self, id: &WorkspaceId, expected_revision: u64) -> Result<()>;
    /// Save a Ready descriptor seen by reconnecting. An unknown ID gets a new
    /// record with no request; a known one may only refresh the same target.
    fn save(&self, descriptor: &BackendDescriptor) -> Result<()>;
    /// Load the record for `id`.
    fn record(&self, id: &WorkspaceId) -> Result<Option<MachineRecord>>;
    /// Load every record without querying or discovering provider resources.
    fn records(&self) -> Result<Vec<MachineRecord>>;
    /// The descriptor of a Ready target.
    fn get(&self, id: &WorkspaceId) -> Result<Option<BackendDescriptor>> {
        Ok(self
            .record(id)?
            .and_then(|record| record.descriptor().cloned()))
    }
    /// Descriptors of every Ready target.
    fn list(&self) -> Result<Vec<BackendDescriptor>> {
        Ok(self
            .records()?
            .into_iter()
            .filter_map(|record| record.descriptor().cloned())
            .collect())
    }
}
