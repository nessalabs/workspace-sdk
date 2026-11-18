//! Agent37 instances as attached workspaces: the instance is made, started
//! and stopped by its owner through [`super::Agent37Machines`], and this
//! provider runs the SDK's supervisor in it and opens connections into it.
//! A sleeping instance it wakes, as any request to the instance would: that
//! is using it, not changing its lifecycle.
use super::{
    api::Api, config::Agent37Config, identity::Agent37Identity, machines::valid_instance,
    target::Agent37Target,
};
use crate::{
    domain::*,
    ports::{IBackend, IProvider},
    providers::{
        managed::{self, validate_remote_path, ManagedBackend},
        SupervisorDeployment,
    },
    Error, Result,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// The account Agent37's system templates run as, and sign in to.
const USER: &str = "node";

/// Attach options for an instance the caller made and made ready.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent37AttachOptions {
    /// The instance's id.
    pub instance_id: String,
    /// Absolute, normalized directory of the workspace inside the instance.
    pub root: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Locator {
    pub version: u32,
    pub instance: String,
    pub root: String,
    pub control: String,
    pub supervisor: String,
}

/// Agent37 instances, attached. Create, stop and delete are not offered:
/// the instance's owner makes, wakes and stops it with
/// [`super::Agent37Machines`], whose key makes a retry find the first
/// instance, and attaches it here. This provider holds only what reading an
/// instance and signing in to it need.
#[derive(Clone, Debug)]
pub struct Agent37Provider {
    id: ProviderId,
    api: Api,
    identity: Arc<Agent37Identity>,
    supervisor: String,
}
impl Agent37Provider {
    /// Instances of the workspace `config` reaches, signed in to with
    /// `identity`, which must be registered there, running the supervisor
    /// that `supervisor` names. Only a preinstalled supervisor is
    /// supported: an Agent37 template carries it.
    ///
    /// # Errors
    /// [`Error::Unsupported`] for a supervisor to upload.
    pub fn new(
        config: Agent37Config,
        identity: Agent37Identity,
        supervisor: SupervisorDeployment,
    ) -> Result<Self> {
        let SupervisorDeployment::Preinstalled { executable } = supervisor else {
            return Err(Error::Unsupported(
                "Agent37 runs a supervisor its template carries".into(),
            ));
        };
        validate_remote_path(&executable)?;
        Ok(Self {
            id: ProviderId::new("agent37").expect("static provider identity"),
            api: Api::new(config).map_err(|_| Error::State("no HTTP client for Agent37".into()))?,
            identity: Arc::new(identity),
            supervisor: executable,
        })
    }
    /// The same provider under another registration id.
    pub fn with_id(mut self, id: ProviderId) -> Self {
        self.id = id;
        self
    }
    fn locator(&self, id: &WorkspaceId, options: Agent37AttachOptions) -> Result<Locator> {
        valid_instance(&options.instance_id)
            .map_err(|_| Error::InvalidInput("not an Agent37 instance id".into()))?;
        validate_remote_path(&options.root)?;
        let control = managed::control_beside(&options.root, id, validate_remote_path)?;
        Ok(Locator {
            version: 1,
            instance: options.instance_id,
            root: options.root,
            control,
            supervisor: self.supervisor.clone(),
        })
    }
    fn backend(&self, reference: WorkspaceRef, locator: Locator) -> ManagedBackend {
        ManagedBackend {
            descriptor: BackendDescriptor {
                reference,
                root: locator.root.clone(),
                control_root: locator.control.clone(),
                isolation: Isolation::ProviderSandbox,
                ownership: Ownership::Attached,
                // Derived from the backend's accessors when read.
                capabilities: Capabilities::default(),
                admission: None,
            },
            target: Arc::new(Agent37Target {
                api: self.api.clone(),
                identity: self.identity.clone(),
                user: USER,
                locator,
                private_link: super::idle_slot::IdleSlot::new(super::target::PRIVATE_IDLE),
            }),
        }
    }
}

#[async_trait]
impl IProvider for Agent37Provider {
    fn id(&self) -> ProviderId {
        self.id.clone()
    }
    fn check_create(&self, _request: &CreateRequest) -> Result<()> {
        Err(Error::Unsupported(
            "make Agent37 instances with Agent37Machines and attach them".into(),
        ))
    }
    fn check_attach(&self, request: &AttachRequest) -> Result<()> {
        self.locator(&request.workspace_id, request.config.decode()?)
            .map(drop)
    }
    async fn create(&self, request: CreateRequest) -> Result<Arc<dyn IBackend>> {
        self.check_create(&request)?;
        Err(Error::Unsupported("Agent37 create".into()))
    }
    async fn attach(&self, request: AttachRequest) -> Result<Arc<dyn IBackend>> {
        let options: Agent37AttachOptions = request.config.decode()?;
        let locator = self.locator(&request.workspace_id, options)?;
        let reference = WorkspaceRef {
            version: 1,
            id: request.workspace_id,
            provider: self.id(),
            locator: WorkspaceLocator::from_locator(&locator)?,
        };
        let backend = self.backend(reference, locator);
        backend.target.ensure().await?;
        backend.initialize(true).await?;
        backend.ensure_ready().await?;
        Ok(Arc::new(backend))
    }
    async fn connect(&self, reference: &WorkspaceRef) -> Result<Arc<dyn IBackend>> {
        let locator: Locator = managed::decode_reference(reference, &self.id)?;
        let expected = self.locator(
            &reference.id,
            Agent37AttachOptions {
                instance_id: locator.instance.clone(),
                root: locator.root.clone(),
            },
        )?;
        managed::require_same(&locator, &expected, "Agent37")?;
        let backend = self.backend(reference.clone(), locator);
        // Missing state is never initialized or replaced on reconnection.
        backend.ensure_ready().await?;
        Ok(Arc::new(backend))
    }
}
