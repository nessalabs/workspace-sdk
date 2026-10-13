use crate::{domain::*, Result};

/// Persistence interface for stable workspace identity and reconnect descriptions.
///
/// Applications can implement this with files, a database, or memory. The role
/// is workspace persistence, not file storage; [`crate::infrastructure::FileStateRepository`]
/// is one implementation rather than a requirement of the domain.
pub trait IWorkspaceRepository: Send + Sync {
    /// Atomically reserve a stable request identity before creating resources.
    /// Reusing an ID with a different identity must return Conflict.
    fn reserve(&self, id: &WorkspaceId, identity: &str) -> Result<()>;
    /// Persist a descriptor without rebinding an existing ID to another target.
    fn save(&self, descriptor: &BackendDescriptor) -> Result<()>;
    /// Load a saved description by application workspace identity.
    fn get(&self, id: &WorkspaceId) -> Result<Option<BackendDescriptor>>;
    /// List recorded descriptions without querying or discovering provider resources.
    fn list(&self) -> Result<Vec<BackendDescriptor>>;
}
