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
    /// The provider checks the request first, without any effect. Then the ID
    /// is reserved and saved as Provisioning before the provider is asked for
    /// anything. A retry of the same request continues from the saved state:
    /// Provisioning asks the provider again with the same key, Ready
    /// reconnects, Refused returns the saved refusal. A different request for
    /// the same ID, including an attach, is a conflict.
    pub async fn create(
        &self,
        provider: &ProviderId,
        mut request: CreateRequest,
    ) -> Result<OwnedWorkspace> {
        let adapter = self.provider(provider)?;
        adapter.check_create(&request)?;
        let identity = request_identity(&("create", provider, &request))?;
        let record = self.repository.begin(
            &request.workspace_id,
            provider,
            &identity,
            MachineState::Provisioning {
                key: request.provision_key(),
            },
        )?;
        // The provider gets the key saved in the record, never a recomputed one.
        if let MachineState::Provisioning { key } = &record.state {
            request.key = Some(key.clone());
        }
        let workspace = self
            .finish(
                provider,
                record,
                Ownership::SdkOwned,
                adapter.create(request),
            )
            .await?;
        OwnedWorkspace::from_owned(workspace)
    }
    /// Attach to an existing target. The returned handle cannot stop or destroy it.
    ///
    /// The provider checks the request first, then the ID is reserved and
    /// saved as Attaching. A retry of the same request continues from the saved
    /// state; a different request conflicts.
    pub async fn attach(&self, provider: &ProviderId, request: AttachRequest) -> Result<Workspace> {
        let adapter = self.provider(provider)?;
        adapter.check_attach(&request)?;
        let identity = request_identity(&("attach", provider, &request))?;
        let record = self.repository.begin(
            &request.workspace_id,
            provider,
            &identity,
            MachineState::Attaching,
        )?;
        self.finish(
            provider,
            record,
            Ownership::Attached,
            adapter.attach(request),
        )
        .await
    }
    /// Delete a record that never reached an outcome, read at
    /// `expected_revision` with [`Self::record`].
    ///
    /// Call it only after the provider has confirmed it holds nothing for this
    /// ID, for example after deleting what the lost create made: the next
    /// create starts again with the same key, so a target the provider still
    /// holds would be found again rather than leaked, but an attach or a
    /// different create would lose track of it. Ready and Refused records are
    /// kept, and a record that changed since it was read is a conflict.
    pub fn forget_unfinished(&self, id: &WorkspaceId, expected_revision: u64) -> Result<()> {
        self.repository.forget(id, expected_revision)
    }
    /// Load the saved record for `id`, whatever state it is in.
    pub fn record(&self, id: &WorkspaceId) -> Result<Option<MachineRecord>> {
        self.repository.record(id)
    }
    /// Continue a reserved record to its outcome. `open` is the provider call
    /// for the record's request; it runs only while the record is pending.
    async fn finish(
        &self,
        provider: &ProviderId,
        record: MachineRecord,
        ownership: Ownership,
        open: impl Future<Output = Result<Arc<dyn IBackend>>>,
    ) -> Result<Workspace> {
        // `begin` only returns a pending record of the request's own kind.
        if !record.state.is_pending() {
            return self.outcome(record).await;
        }
        let id = record.workspace_id.clone();
        let backend = match open.await {
            Ok(backend) => backend,
            Err(Error::Refused(refusal)) => {
                let refused = MachineState::Refused {
                    refusal: (*refusal).clone(),
                };
                return match self.settle(&record, refused)? {
                    // Another caller finished the same request first.
                    Some(winner) => self.outcome(winner).await,
                    None => Err(Error::Refused(refusal)),
                };
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
        let ready = MachineState::Ready {
            descriptor: descriptor.clone(),
        };
        if let Some(winner) = self.settle(&record, ready)? {
            // Another caller finished the same request first; use its outcome.
            return self.outcome(winner).await;
        }
        backend.ensure_ready().await?;
        Ok(Workspace { backend })
    }
    /// The workspace a settled record stands for.
    async fn outcome(&self, record: MachineRecord) -> Result<Workspace> {
        match record.state {
            MachineState::Ready { descriptor } => self.resume(&descriptor).await,
            MachineState::Refused { refusal } => Err(Error::Refused(Box::new(refusal))),
            _ => Err(Error::State("the saved record has no outcome yet".into())),
        }
    }
    /// Save the outcome of the request that reserved `reserved`. If another
    /// caller of the same reservation saved one first, return its record. A
    /// record made after this one was forgotten, even for the same request, is
    /// never treated as this call's outcome.
    fn settle(
        &self,
        reserved: &MachineRecord,
        next: MachineState,
    ) -> Result<Option<MachineRecord>> {
        let id = &reserved.workspace_id;
        let forgotten = || {
            Error::Conflict(format!(
                "workspace {id} was forgotten while its request ran"
            ))
        };
        match self.repository.advance(id, reserved.revision, next) {
            Ok(_) => Ok(None),
            Err(Error::Conflict(message)) => match self.repository.record(id)? {
                Some(record) if !record.continues(reserved) => Err(forgotten()),
                Some(record) if !record.state.is_pending() => Ok(Some(record)),
                _ => Err(Error::Conflict(message)),
            },
            Err(Error::NotFound(_)) => Err(forgotten()),
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
    /// Reconnect to a target this SDK created, with its stop and destroy
    /// operations. The reference must be the one saved in this SDK's Ready
    /// record for a create; anything else, including an attached target, is
    /// refused.
    pub async fn connect_owned(&self, reference: &WorkspaceRef) -> Result<OwnedWorkspace> {
        let saved = self.owned_record(&reference.id)?;
        if saved.reference != *reference {
            return Err(Error::Conflict(format!(
                "workspace {} is saved with a different reference",
                reference.id
            )));
        }
        OwnedWorkspace::from_owned(self.connect(reference).await?)
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
        let saved = self.owned_record(id)?;
        OwnedWorkspace::from_owned(self.connect(&saved.reference).await?)
    }
    /// The saved Ready descriptor of a target this SDK created. A target
    /// only reconnected here, whatever ownership its locator claims, is not one.
    pub(crate) fn owned_record(&self, id: &WorkspaceId) -> Result<BackendDescriptor> {
        let record = self
            .repository
            .record(id)?
            .ok_or_else(|| Error::NotFound(format!("workspace {id}")))?;
        if !record.created_here() {
            return Err(Error::Conflict(format!(
                "workspace {id} was not created by this SDK; it cannot be stopped or destroyed here"
            )));
        }
        record
            .descriptor()
            .cloned()
            .ok_or_else(|| Error::State(format!("workspace {id} has no target")))
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
