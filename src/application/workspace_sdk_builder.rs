use super::WorkspaceSdk;
use crate::{
    domain::ProviderId,
    ports::{IProvider, IWorkspaceRepository},
    Error, Result,
};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Dependency-injection builder for the workspace application facade.
///
/// Callers choose provider adapters and persistence; no global registry or
/// implicit cloud credentials are created.
pub struct WorkspaceSdkBuilder {
    providers: BTreeMap<ProviderId, Arc<dyn IProvider>>,
    repository: Arc<dyn IWorkspaceRepository>,
}
impl WorkspaceSdkBuilder {
    /// Create a builder with the repository that will retain workspace identities.
    pub fn new(repository: Arc<dyn IWorkspaceRepository>) -> Self {
        Self {
            providers: BTreeMap::new(),
            repository,
        }
    }
    /// Register an adapter under its provider identity; duplicate IDs are rejected.
    pub fn provider(mut self, provider: Arc<dyn IProvider>) -> Result<Self> {
        let id = provider.id();
        if self.providers.insert(id.clone(), provider).is_some() {
            return Err(Error::Conflict(format!(
                "provider already registered: {id}"
            )));
        }
        Ok(self)
    }
    /// Build the facade, requiring at least one registered provider.
    pub fn build(self) -> Result<WorkspaceSdk> {
        if self.providers.is_empty() {
            return Err(Error::InvalidInput("register at least one provider".into()));
        }
        Ok(WorkspaceSdk {
            providers: self.providers,
            repository: self.repository,
        })
    }
}
