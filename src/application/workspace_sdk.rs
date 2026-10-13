use super::{Workspace, WorkspaceSdkBuilder};
use crate::{
    domain::{BackendDescriptor, OpenRequest, ProviderId, WorkspaceId, WorkspaceRef},
    ports::{IProvider, IWorkspaceRepository},
    Error, Result,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Application facade coordinating workspace identity through injected interfaces.
///
/// The registry selects an [`IProvider`] and the [`IWorkspaceRepository`] keeps
/// durable references and request reservations. This service is not a cloud
/// account, process supervisor, or global mutable workspace entity.
pub struct WorkspaceSdk {
    pub(super) providers: BTreeMap<ProviderId, Arc<dyn IProvider>>,
    pub(super) repository: Arc<dyn IWorkspaceRepository>,
}
impl WorkspaceSdk {
    /// Start configuration with an application-owned persistence implementation.
    pub fn builder(repository: Arc<dyn IWorkspaceRepository>) -> WorkspaceSdkBuilder {
        WorkspaceSdkBuilder::new(repository)
    }
    /// Open a new or attached target, reserving request identity before side effects.
    /// Retries of the same identity reconnect; incompatible reuse returns a conflict.
    pub async fn open(&self, provider: &ProviderId, request: OpenRequest) -> Result<Workspace> {
        let adapter = self
            .providers
            .get(provider)
            .ok_or_else(|| Error::NotFound(format!("provider {provider}")))?;
        let identity = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(provider, &request))?)
        );
        self.repository.reserve(&request.workspace_id, &identity)?;
        if let Some(saved) = self.repository.get(&request.workspace_id)? {
            let workspace = self.connect(&saved.reference).await?;
            workspace.ensure_ready().await?;
            return Ok(workspace);
        }
        let requested_id = request.workspace_id.clone();
        let backend = adapter.open(request).await?;
        let descriptor = backend.descriptor();
        if descriptor.reference.id != requested_id
            || &descriptor.reference.provider != provider
            || descriptor.reference.version != 1
        {
            return Err(Error::State(
                "provider returned a workspace outside the requested identity".into(),
            ));
        }
        // Persist the creation receipt before a readiness timeout can interrupt us.
        self.repository.save(&descriptor)?;
        backend.ensure_ready().await?;
        Ok(Workspace { backend })
    }
    /// Reconnect using a persisted reference and verify that the provider preserves identity.
    pub async fn connect(&self, reference: &WorkspaceRef) -> Result<Workspace> {
        if reference.version != 1 {
            return Err(Error::Unsupported("workspace reference version".into()));
        }
        let provider = self
            .providers
            .get(&reference.provider)
            .ok_or_else(|| Error::NotFound(format!("provider {}", reference.provider)))?;
        let backend = provider.connect(reference).await?;
        let descriptor = backend.descriptor();
        if descriptor.reference != *reference {
            return Err(Error::State(
                "provider changed the persisted workspace identity during reconnect".into(),
            ));
        }
        self.repository.save(&descriptor)?;
        Ok(Workspace { backend })
    }
    /// Load a workspace reference from the repository and reconnect through its provider.
    pub async fn workspace(&self, id: &WorkspaceId) -> Result<Workspace> {
        let descriptor = self
            .repository
            .get(id)?
            .ok_or_else(|| Error::NotFound(format!("workspace {id}")))?;
        self.connect(&descriptor.reference).await
    }
    /// List persisted descriptors without polling providers or discovering unrelated resources.
    pub fn list(&self) -> Result<Vec<BackendDescriptor>> {
        self.repository.list()
    }
}
