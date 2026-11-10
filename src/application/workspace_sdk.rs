use super::{OwnedWorkspace, Workspace, WorkspaceSdkBuilder};
use crate::{
    domain::{
        AttachRequest, BackendDescriptor, CreateRequest, MachineRecord, MachineState, Ownership,
        ProviderId, WorkspaceId, WorkspaceRef,
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
    /// Create a new SDK-owned target.
    ///
    /// The ID is reserved and saved as Provisioning before the provider is
    /// asked for anything. A retry of the same request continues from the saved
    /// state: Provisioning asks the provider again with the same key, Ready
    /// reconnects, Refused returns the saved refusal. A different request for
    /// the same ID, including an attach, is a conflict.
    pub async fn create(
        &self,
        provider: &ProviderId,
        request: CreateRequest,
    ) -> Result<OwnedWorkspace> {
        let adapter = self.provider(provider)?;
        let id = request.workspace_id.clone();
        // Hashed as before the create/attach split, so saved creates still match.
        let identity = request_identity(&(provider, &request))?;
        let key = request.provision_key();
        let record = self
            .repository
            .begin(&id, &identity, MachineState::Provisioning { key })?;
        let workspace = self
            .finish(provider, record, Ownership::SdkOwned, |state| async move {
                if !matches!(state, MachineState::Provisioning { .. }) {
                    return Err(Error::State("create found an attach record".into()));
                }
                adapter.create(request).await
            })
            .await?;
        Ok(OwnedWorkspace { workspace })
    }
    /// Attach to an existing target. The returned handle cannot stop or destroy it.
    ///
    /// The ID is reserved and saved as Attaching first. A retry of the same
    /// request continues from the saved state; a different request conflicts.
    pub async fn attach(&self, provider: &ProviderId, request: AttachRequest) -> Result<Workspace> {
        let adapter = self.provider(provider)?;
        let id = request.workspace_id.clone();
        let identity = request_identity(&("attach", provider, &request))?;
        let record = self
            .repository
            .begin(&id, &identity, MachineState::Attaching)?;
        self.finish(provider, record, Ownership::Attached, |state| async move {
            let MachineState::Attaching = state else {
                return Err(Error::State("attach found a create record".into()));
            };
            adapter.attach(request).await
        })
        .await
    }
    /// Delete a record that never reached an outcome, after the provider has
    /// confirmed it holds nothing for it. The next create starts again with the
    /// same key. Ready and Refused records are kept.
    pub fn forget_provisioning(&self, id: &WorkspaceId) -> Result<()> {
        let record = self
            .repository
            .record(id)?
            .ok_or_else(|| Error::NotFound(format!("workspace {id}")))?;
        self.repository.forget(id, record.revision)
    }
    /// Load the saved record for `id`, whatever state it is in.
    pub fn record(&self, id: &WorkspaceId) -> Result<Option<MachineRecord>> {
        self.repository.record(id)
    }
    async fn finish<F, Fut>(
        &self,
        provider: &ProviderId,
        record: MachineRecord,
        ownership: Ownership,
        open: F,
    ) -> Result<Workspace>
    where
        F: FnOnce(MachineState) -> Fut,
        Fut: Future<Output = Result<Arc<dyn IBackend>>>,
    {
        let id = record.workspace_id.clone();
        let pending = match record.state {
            MachineState::Ready { descriptor } => return self.resume(&descriptor).await,
            MachineState::Refused { refusal } => return Err(Error::Refused(Box::new(refusal))),
            pending => pending,
        };
        let backend = match open(pending).await {
            Ok(backend) => backend,
            Err(Error::Refused(refusal)) => {
                self.settle(
                    &id,
                    record.revision,
                    MachineState::Refused {
                        refusal: (*refusal).clone(),
                    },
                )?;
                return Err(Error::Refused(refusal));
            }
            // No outcome: the record stays pending for a retry.
            Err(error) => return Err(error),
        };
        let descriptor = backend.descriptor();
        if descriptor.reference.id != id
            || &descriptor.reference.provider != provider
            || descriptor.reference.version != 1
            || descriptor.ownership != ownership
        {
            return Err(Error::State(
                "provider returned a workspace outside the requested identity".into(),
            ));
        }
        // Persist the outcome before a readiness timeout can interrupt us.
        if let Some(winner) = self.settle(
            &id,
            record.revision,
            MachineState::Ready {
                descriptor: descriptor.clone(),
            },
        )? {
            // Another caller finished the same request first; use its outcome.
            return match winner.state {
                MachineState::Ready { descriptor } => self.resume(&descriptor).await,
                MachineState::Refused { refusal } => Err(Error::Refused(Box::new(refusal))),
                _ => Err(Error::State("concurrent create left no outcome".into())),
            };
        }
        backend.ensure_ready().await?;
        Ok(Workspace { backend })
    }
    /// Save an outcome. If another caller saved one first, return its record.
    fn settle(
        &self,
        id: &WorkspaceId,
        revision: u64,
        next: MachineState,
    ) -> Result<Option<MachineRecord>> {
        match self.repository.advance(id, revision, next) {
            Ok(_) => Ok(None),
            Err(Error::Conflict(message)) => match self.repository.record(id)? {
                Some(record) if record.revision != revision => Ok(Some(record)),
                _ => Err(Error::Conflict(message)),
            },
            Err(error) => Err(error),
        }
    }
    async fn resume(&self, descriptor: &BackendDescriptor) -> Result<Workspace> {
        let workspace = self.connect(&descriptor.reference).await?;
        workspace.ensure_ready().await?;
        Ok(workspace)
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
