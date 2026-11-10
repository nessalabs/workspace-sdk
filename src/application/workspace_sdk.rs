use super::{OwnedWorkspace, Workspace, WorkspaceSdkBuilder};
use crate::{
    domain::{
        AttachRequest, BackendDescriptor, CreateRequest, Ownership, ProviderId, WorkspaceId,
        WorkspaceRef,
    },
    ports::{IBackend, IProvider, IWorkspaceRepository},
    Error, Result,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::future::Future;
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
    /// Create a new SDK-owned target, reserving request identity before side effects.
    ///
    /// Retries of the same request reconnect to the saved target; a different
    /// request for the same workspace ID, including an attach, is a conflict.
    pub async fn create(
        &self,
        provider: &ProviderId,
        request: CreateRequest,
    ) -> Result<OwnedWorkspace> {
        let adapter = self.provider(provider)?;
        let id = request.workspace_id.clone();
        let identity = request_identity(&("create", provider, &request))?;
        let workspace = self
            .admit(provider, &id, &identity, Ownership::SdkOwned, async {
                adapter.create(request).await
            })
            .await?;
        Ok(OwnedWorkspace { workspace })
    }
    /// Attach to an existing target, reserving request identity before side effects.
    ///
    /// The returned handle cannot stop or destroy the target. Retries of the same
    /// request reconnect; a different request for the same workspace ID conflicts.
    pub async fn attach(&self, provider: &ProviderId, request: AttachRequest) -> Result<Workspace> {
        let adapter = self.provider(provider)?;
        let id = request.workspace_id.clone();
        let identity = request_identity(&("attach", provider, &request))?;
        self.admit(provider, &id, &identity, Ownership::Attached, async {
            adapter.attach(request).await
        })
        .await
    }
    async fn admit(
        &self,
        provider: &ProviderId,
        id: &WorkspaceId,
        identity: &str,
        ownership: Ownership,
        open: impl Future<Output = Result<Arc<dyn IBackend>>>,
    ) -> Result<Workspace> {
        self.repository.reserve(id, identity)?;
        if let Some(saved) = self.repository.get(id)? {
            let workspace = self.connect(&saved.reference).await?;
            workspace.ensure_ready().await?;
            return Ok(workspace);
        }
        let backend = open.await?;
        let descriptor = backend.descriptor();
        if descriptor.reference.id != *id
            || &descriptor.reference.provider != provider
            || descriptor.reference.version != 1
            || descriptor.ownership != ownership
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
    fn provider(&self, provider: &ProviderId) -> Result<&Arc<dyn IProvider>> {
        self.providers
            .get(provider)
            .ok_or_else(|| Error::NotFound(format!("provider {provider}")))
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
    /// Reconnect to a target this SDK created, with its stop and destroy operations.
    /// Refuses an attached target.
    pub async fn connect_owned(&self, reference: &WorkspaceRef) -> Result<OwnedWorkspace> {
        OwnedWorkspace::try_from(self.connect(reference).await?)
    }
    /// Load a workspace reference from the repository and reconnect through its provider.
    pub async fn workspace(&self, id: &WorkspaceId) -> Result<Workspace> {
        let descriptor = self
            .repository
            .get(id)?
            .ok_or_else(|| Error::NotFound(format!("workspace {id}")))?;
        self.connect(&descriptor.reference).await
    }
    /// Load a saved target this SDK created, with its stop and destroy operations.
    pub async fn owned_workspace(&self, id: &WorkspaceId) -> Result<OwnedWorkspace> {
        OwnedWorkspace::try_from(self.workspace(id).await?)
    }
    /// List persisted descriptors without polling providers or discovering unrelated resources.
    pub fn list(&self) -> Result<Vec<BackendDescriptor>> {
        self.repository.list()
    }
}

fn request_identity(request: &impl Serialize) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(request)?)
    ))
}
