use super::machines::{
    AdmissionRefusal, AdmissionRejection, BoatMachines, Budget, Created, Disposition,
    EnvironmentIdentity, MachineFailure, MachineSpec,
};
use super::{error::error, target::SandboxTarget, SandboxAttachOptions, SandboxCreateOptions};
use crate::providers::managed::{self, Locator, ManagedBackend};
use crate::providers::SupervisorDeployment;
use crate::{
    domain::*,
    ports::{IBackend, IProvider},
    Error, Result,
};
use async_trait::async_trait;
use sandbox_client::{Api, CreateOptions, CreateSandboxRequest};
use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

/// How long one wake of a stopped workspace may take by default: waiting
/// out an archive, resuming, and becoming ready.
const WAKE: Duration = Duration::from_secs(300);

/// Uses an injected real Api. Construct its configuration explicitly; this
/// adapter never reads API credentials from the process environment.
///
/// Each [`IBackend::connection`](crate::ports::IBackend::connection) to one
/// of its workspaces authorizes a new SSH key on the machine, which Boat
/// cannot revoke; the key's private half is deleted with the connection.
/// Connections need `ssh` and `ssh-keygen` on this machine, and Linux with
/// `/proc` on the workspace's.
///
/// With [`SandboxProvider::with_machines`], every workspace it creates is a
/// Boat machine admitted in full, and a machine that fails admission is
/// refused with its reason.
#[derive(Clone)]
pub struct SandboxProvider {
    id: ProviderId,
    api: Api,
    supervisor: SupervisorDeployment,
    machines: Option<BoatMachines>,
    wake: Budget,
}
impl SandboxProvider {
    /// Assign a registry ID when configuring multiple targets of this provider type.
    pub fn with_id(mut self, id: ProviderId) -> Self {
        self.id = id;
        self
    }

    /// Configure an explicitly installed or supplied native supervisor.
    pub fn with_supervisor(mut self, supervisor: SupervisorDeployment) -> Self {
        self.supervisor = supervisor;
        self
    }

    /// Create every workspace as one of `machines`, admitted in full: its
    /// account's environment, its creation and its receipt are checked, and
    /// a machine that fails is refused ([`Error::Refused`]) with the reason,
    /// and deleted when this creation made it. Creating then takes an
    /// `image` and an `environment` in [`SandboxCreateOptions`], and a
    /// `sandbox_type`.
    ///
    /// Every call then uses the machines' account, in its personal scope:
    /// it replaces the client this provider was made with.
    pub fn with_machines(mut self, machines: BoatMachines) -> Self {
        self.api = machines.api().clone();
        self.machines = Some(machines);
        self
    }

    /// How long waking a stopped workspace may take, waiting out an archive,
    /// resuming and becoming ready included. Five minutes by default.
    pub fn with_wake_budget(mut self, budget: Budget) -> Self {
        self.wake = budget;
        self
    }

    pub fn new(api: Api) -> Self {
        Self {
            id: ProviderId::new(super::branding::DEFAULT_PROVIDER_ID).expect("static ID"),
            api,
            supervisor: SupervisorDeployment::default(),
            machines: None,
            wake: Budget::new(WAKE).expect("a valid budget"),
        }
    }
    fn create_options(&self, request: &CreateRequest) -> Result<SandboxCreateOptions> {
        if request.resources != ResourceRequirements::default() {
            return Err(Error::Unsupported(
                "Boat generic resource provisioning; choose an explicit sandbox_type in SandboxCreateOptions"
                    .into(),
            ));
        }
        if request.config.is_empty() {
            return Err(Error::InvalidInput(
                "SandboxCreateOptions with an explicit root is required".into(),
            ));
        }
        let options: SandboxCreateOptions = request.config.decode()?;
        managed::validate_remote_path(&options.root)?;
        self.supervisor.validate()?;
        if self.machines.is_some() {
            spec(&options)?;
        } else if options.image.is_some() || !options.variables.is_empty() {
            return Err(Error::InvalidInput(
                "an image or variables need a provider with Boat machines".into(),
            ));
        }
        Ok(options)
    }
    /// Make, admit and ready one machine; refuse it with its reason.
    async fn create_machine(
        &self,
        machines: &BoatMachines,
        options: &SandboxCreateOptions,
        key: &ProvisionKey,
    ) -> Result<(String, Admission)> {
        let spec = spec(options)?;
        machines.prepare(&spec).await.map_err(failure)?;
        let created = machines
            .create(&spec, key.as_str())
            .await
            .map_err(failure)?;
        if let Some(rejection) = created.refusal() {
            return Err(refuse(machines, &created, rejection).await);
        }
        match machines
            .ready(&spec, created.vm(), created.admission())
            .await
        {
            Ok(_) => Ok((created.vm().to_owned(), admission(created.admission()))),
            Err(MachineFailure::AdmissionRejected(rejection)) => {
                Err(refuse(machines, &created, &rejection).await)
            }
            Err(other) => Err(failure(other)),
        }
    }
    async fn open(
        &self,
        workspace_id: WorkspaceId,
        root: &str,
        target: String,
        ownership: Ownership,
        admission: Option<Admission>,
    ) -> Result<Arc<dyn IBackend>> {
        let directory = format!("{}/{}", root.trim_end_matches('/'), workspace_id);
        let locator = Locator {
            root: format!("{directory}/project"),
            control: format!("{directory}/control"),
            ownership,
            target,
        };
        let reference = WorkspaceRef {
            version: 1,
            id: workspace_id,
            provider: self.id(),
            locator: WorkspaceLocator::from_locator(&locator)?,
        };
        let creation_receipt = locator.ownership == Ownership::SdkOwned;
        let mut backend = self.backend(reference, locator, creation_receipt);
        backend.descriptor.admission = admission;
        // The stable create key makes a failed bootstrap retryable. Initialization
        // is only permitted here: reconnect must not recreate vanished run claims.
        backend.target.ensure().await?;
        backend.initialize(true).await?;
        Ok(Arc::new(backend))
    }
    fn backend(
        &self,
        reference: WorkspaceRef,
        locator: Locator,
        creation_receipt: bool,
    ) -> ManagedBackend {
        let owned = locator.ownership == Ownership::SdkOwned;
        ManagedBackend {
            descriptor: BackendDescriptor {
                reference: reference.clone(),
                root: locator.root.clone(),
                control_root: locator.control.clone(),
                isolation: Isolation::ProviderSandbox,
                ownership: locator.ownership.clone(),
                capabilities: Capabilities {
                    managed_runs: true,
                    file_transfer: true,
                    stop: owned,
                    destroy: owned,
                    private_ports: false,
                    hosted_ports: true,
                },
                admission: None,
            },
            target: Arc::new(SandboxTarget {
                api: self.api.clone(),
                supervisor: self.supervisor.clone(),
                locator,
                workspace_id: reference.id,
                verified_owned: AtomicBool::new(creation_receipt),
                wake: self.wake.duration(),
            }),
        }
    }
}
#[async_trait]
impl IProvider for SandboxProvider {
    fn id(&self) -> ProviderId {
        self.id.clone()
    }
    fn check_create(&self, request: &CreateRequest) -> Result<()> {
        self.create_options(request).map(drop)
    }
    fn check_attach(&self, request: &AttachRequest) -> Result<()> {
        let options: SandboxAttachOptions = request.config.decode()?;
        managed::validate_remote_path(&options.root)?;
        self.supervisor.validate()
    }
    async fn create(&self, request: CreateRequest) -> Result<Arc<dyn IBackend>> {
        let key = request.provision_key();
        let options = self.create_options(&request)?;
        if let Some(machines) = &self.machines {
            let (vm, admission) = self.create_machine(machines, &options, &key).await?;
            return self
                .open(
                    request.workspace_id,
                    &options.root,
                    vm,
                    Ownership::SdkOwned,
                    Some(admission),
                )
                .await;
        }
        let no_env = options.environment.is_none().then_some(true);
        let created = self
            .api
            .create(
                Some(CreateSandboxRequest {
                    type_: options.sandbox_type,
                    ttl_seconds: Some(options.ttl_seconds),
                    environment: options.environment,
                    no_env,
                    ..CreateSandboxRequest::default()
                }),
                &CreateOptions {
                    idempotency_key: Some(key.as_str().to_owned()),
                    ..CreateOptions::default()
                },
            )
            .await
            .map_err(error)?;
        self.open(
            request.workspace_id,
            &options.root,
            created.sandbox.id,
            Ownership::SdkOwned,
            None,
        )
        .await
    }
    async fn attach(&self, request: AttachRequest) -> Result<Arc<dyn IBackend>> {
        self.check_attach(&request)?;
        let options: SandboxAttachOptions = request.config.decode()?;
        self.api.get(&options.sandbox_id).await.map_err(error)?;
        self.open(
            request.workspace_id,
            &options.root,
            options.sandbox_id,
            Ownership::Attached,
            None,
        )
        .await
    }
    async fn connect(&self, reference: &WorkspaceRef) -> Result<Arc<dyn IBackend>> {
        let locator = managed::decode(reference, &self.id())?;
        let suffix = format!("/{}/project", reference.id);
        if !locator.root.ends_with(&suffix)
            || locator.control != format!("{}/control", locator.root.trim_end_matches("/project"))
        {
            return Err(Error::InvalidInput(
                "Sandbox locator is outside the workspace namespace".into(),
            ));
        }
        Ok(Arc::new(self.backend(reference.clone(), locator, false)))
    }
}

/// The machine `options` ask for.
fn spec(options: &SandboxCreateOptions) -> Result<MachineSpec> {
    let (Some(image), Some(environment), Some(machine_type)) =
        (&options.image, &options.environment, &options.sandbox_type)
    else {
        return Err(Error::InvalidInput(
            "a Boat machine needs an image, an environment and a sandbox_type".into(),
        ));
    };
    MachineSpec::new(
        image.as_str(),
        environment.as_str(),
        machine_type.as_str(),
        options.ttl_seconds,
        options.variables.clone(),
    )
    .map_err(|_| {
        Error::InvalidInput(
            "a Boat machine's image, environment and type are plain names, its lifetime \
             is above zero, and its variables are shell names with short values"
                .into(),
        )
    })
}

/// What the SDK saves of the environment a machine was admitted under.
fn admission(identity: &EnvironmentIdentity) -> Admission {
    Admission {
        environment_id: identity.id().into(),
        version_id: identity.version_id().into(),
        version: identity.version().to_string(),
        name: identity.name().into(),
    }
}

/// A refused machine, deleted first when this creation made it.
async fn refuse(
    machines: &BoatMachines,
    created: &Created,
    rejection: &AdmissionRejection,
) -> Error {
    let cleanup = match created.disposition() {
        Disposition::RetainTarget => RefusalCleanup::Kept,
        // Boat never confirms a deletion, and an answer that cannot be read
        // may follow one it made, so any deletion asked ends uncertain.
        Disposition::RollbackNewVm => {
            let _ = machines.roll_back(created).await;
            RefusalCleanup::Unknown
        }
    };
    let kind = match rejection.cause() {
        AdmissionRefusal::MissingEnvironment
        | AdmissionRefusal::WrongEnvironment
        | AdmissionRefusal::MissingVersion
        | AdmissionRefusal::WrongVersion
        | AdmissionRefusal::EnvironmentChanged
        | AdmissionRefusal::EnvironmentUnavailable => RefusalKind::EnvironmentMismatch,
        AdmissionRefusal::WipePending => RefusalKind::PendingWipe,
        AdmissionRefusal::InvalidReceipt
        | AdmissionRefusal::ReceiptUnavailable
        | AdmissionRefusal::LifetimeNotEnforced => RefusalKind::NotAdmitted,
    };
    Error::Refused(Box::new(Refusal {
        kind,
        message: rejection.to_string(),
        cleanup,
        target: Some(created.vm().to_owned()),
        admission: Some(admission(created.admission())),
    }))
}

/// A machine failure with no refusal: the record stays pending, and asking
/// again with the same key finishes it.
fn failure(failure: MachineFailure) -> Error {
    let provider = |message: &str| Error::Provider {
        provider: super::branding::NAME.into(),
        message: message.into(),
    };
    match failure {
        MachineFailure::InvalidInput => {
            Error::InvalidInput("Boat could not take the machine's spec or key".into())
        }
        MachineFailure::Missing => Error::NotFound("Boat does not know the image".into()),
        MachineFailure::EnvironmentUnavailable => Error::State(
            "the account's environment is ambiguous or passes more than agent credentials; \
             fix or delete it"
                .into(),
        ),
        MachineFailure::ProvisioningUncertain => {
            provider("Boat may have made the machine; asking again finishes it")
        }
        MachineFailure::CleanupUncertain => provider("Boat did not confirm a deletion"),
        MachineFailure::AdmissionRejected(rejection) => provider(&rejection.to_string()),
        MachineFailure::Provider => provider("Boat failed, refused or timed out"),
    }
}
