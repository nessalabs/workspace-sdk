//! Strict provider receipts. Nothing runs on, or is handed out for, a VM that
//! has not passed them: the account's environment posture, the creation
//! acknowledgement and the complete info receipt are each read as raw JSON so
//! a new credential or access field cannot be dropped by a lenient decoder.
use super::{
    account::{Boat, Created, Disposition},
    diagnostics::{self, Diagnostic, Stage},
    environment::{
        AdmissionRefusal, AdmissionRejection, EnvironmentConfiguration, EnvironmentIdentity,
        ObservedEnvironment,
    },
    events::MachineEvent,
    spec::{MachineFailure, MachineSpec},
};
use sandbox_client::{
    CreateSandboxRequest, CreateSandboxResponse, SandboxInfoResponse, SandboxState,
};
use serde_json::Value;
use tokio::time::Instant;

const SANDBOX_FIELDS: &[&str] = &[
    "id",
    "name",
    "state",
    "error",
    "dataExpiredAt",
    "stoppedAt",
    "labels",
    "health",
    "healthReason",
    "degradedSince",
    "hydrated",
    "hydratedAt",
    "hydration",
    "type",
    "vcpu",
    "memoryGB",
    "billingMultiplier",
    "machineProvider",
    "url",
    "ip",
    "sshEndpoint",
    "createdAt",
    "updatedAt",
    "archiveAfter",
    "ttlSeconds",
    "desktopAvailable",
    "desktopUrl",
    "snapshots",
    "monitoring",
    "snapshotAvailable",
    "snapshotCompletedAt",
    "snapshotVerifiedAt",
    "lastSnapshotAttemptAt",
    "lastSnapshotStatus",
    "lastReboot",
    "subdomain",
    "team",
    "createdBy",
    "createdById",
    "access",
    "holdsCreatorLogins",
    "visibility",
    "wipePendingUntilRestart",
    "setupStatus",
    "setupError",
    "environment",
    "environmentVersion",
    "stop",
];

fn invalid_receipt(boat: &Boat, diagnostic: Diagnostic) -> MachineFailure {
    diagnostic.emit(boat);
    MachineFailure::AdmissionRejected(AdmissionRejection::new(
        AdmissionRefusal::InvalidReceipt,
        None,
    ))
}

/// The account's environment named by `spec`, admitted only in the one
/// supported posture: agent credentials passed, nothing else injected.
pub(super) async fn environment(
    boat: &Boat,
    spec: &MachineSpec,
) -> Result<EnvironmentIdentity, MachineFailure> {
    let response = boat
        .api
        .environments()
        .await
        .map_err(|_| MachineFailure::Provider)?;
    if response.extra.len() != 2
        || response.extra.get("ok") != Some(&Value::Bool(true))
        || response.extra.get("type").and_then(Value::as_str) != Some("environments.list")
    {
        return Err(MachineFailure::EnvironmentUnavailable);
    }
    let mut matches = response
        .environments
        .iter()
        .filter(|e| e.name == spec.environment());
    let env = matches
        .next()
        .ok_or(MachineFailure::EnvironmentUnavailable)?;
    if matches.next().is_some() {
        return Err(MachineFailure::EnvironmentUnavailable);
    }
    let proxy_ai_credentials = env
        .extra
        .get("proxyAiCredentials")
        .and_then(Value::as_bool)
        .ok_or(MachineFailure::EnvironmentUnavailable)?;
    EnvironmentConfiguration {
        safe_for_third_parties: env.safe_for_third_parties,
        pass_github: env.pass_github,
        pass_secrets: env.pass_secrets,
        pass_sandbox_credentials: env.pass_sandbox_credentials,
        pass_agents_credentials: env.pass_agents_credentials,
        has_contents: !env.env_contents.is_empty(),
        has_secret_files: !env.secret_files.is_empty(),
        has_repositories: env
            .selected_repositories
            .as_ref()
            .is_some_and(|r| !r.is_empty()),
        owner_default_profile: env.extra.get("credentialProfileId") == Some(&Value::Null),
        proxy_ai_credentials,
    }
    .admit()?;
    // Boat's published contract: explicit null chooses the owner's default;
    // proxyAiCredentials applies only to safe-for-third-parties environments.
    // Personal-scope ResourceAccess is an object, unlike a VM's string access.
    // Verify the provider's computed flags agree with its declared configuration.
    if env.extra.get("access") != Some(&serde_json::json!({"restricted":false,"members":[]}))
        || env.extra.get("effective")
            != Some(&serde_json::json!({
                "passGithub":env.pass_github,"passSecrets":env.pass_secrets,
                "passSandboxCredentials":env.pass_sandbox_credentials,"passAgentsCredentials":env.pass_agents_credentials
            }))
        || env.extra.keys().any(|key| {
            !matches!(
                key.as_str(),
                "credentialProfileId"
                    | "proxyAiCredentials"
                    | "access"
                    | "effective"
                    | "latestVersionNumber"
                    | "safetyAnsweredAt"
            )
        })
        || env.versions.iter().any(|v| !v.extra.is_empty())
    {
        return Err(MachineFailure::EnvironmentUnavailable);
    }
    if env.extra.get("safetyAnsweredAt").is_some_and(|v| {
        !v.is_null()
            && !v
                .as_str()
                .is_some_and(|s| s.len() <= 64 && !s.chars().any(char::is_control))
    }) {
        return Err(MachineFailure::EnvironmentUnavailable);
    }
    let id = env
        .latest_version_id
        .as_ref()
        .ok_or(MachineFailure::EnvironmentUnavailable)?;
    let mut versions = env.versions.iter().filter(|v| &v.id == id);
    let version = versions
        .next()
        .ok_or(MachineFailure::EnvironmentUnavailable)?;
    if versions.next().is_some()
        || env
            .versions
            .iter()
            .filter(|v| v.version_number == version.version_number)
            .count()
            != 1
        || env.extra.get("latestVersionNumber").and_then(Value::as_u64)
            != Some(version.version_number)
    {
        return Err(MachineFailure::EnvironmentUnavailable);
    }
    EnvironmentIdentity::new(
        env.id.clone(),
        env.name.clone(),
        id.clone(),
        u32::try_from(version.version_number)
            .map_err(|_| MachineFailure::EnvironmentUnavailable)?,
    )
}

/// A VM id Boat could have named: 1 to 128 letters, digits, `_` or `-`, so
/// it is safe in a path.
pub(super) fn vm_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn supported_state(sandbox: &sandbox_client::Sandbox) -> bool {
    sandbox.state != SandboxState::Unknown
}

async fn bounded_json(
    boat: &Boat,
    mut response: reqwest::Response,
    stage: Stage,
) -> Result<Value, MachineFailure> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| MachineFailure::Provider)?
    {
        if chunk.len() > 65_536 - bytes.len() {
            return Err(invalid_receipt(
                boat,
                Diagnostic::field(stage, "body", "unknown", "too_large", None),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
        invalid_receipt(
            boat,
            Diagnostic::field(stage, "body", "unknown", "malformed_json", None),
        )
    })?;
    Ok(value)
}

pub(super) async fn create(
    boat: &Boat,
    spec: &MachineSpec,
    key: &str,
    admission: EnvironmentIdentity,
    deadline: Instant,
) -> Result<Created, MachineFailure> {
    let ttl_seconds = spec.lifetime();
    let request = boat
        .request(reqwest::Method::POST, "sandboxes")
        .header("Idempotency-Key", key)
        .json(&CreateSandboxRequest {
            type_: Some(spec.machine_type().into()),
            ttl_seconds: Some(ttl_seconds),
            environment: Some(spec.environment().into()),
            env: Some(spec.variables().clone().into_iter().collect()),
            from: Some(spec.image().into()),
            snapshots: Some(true),
            ..Default::default()
        });
    // Nothing is sent once the budget is spent, so that answer is certain.
    if Instant::now() >= deadline {
        return Err(MachineFailure::Provider);
    }
    // Once the request may have been sent, only an answer that names no VM
    // for certain is anything but uncertain.
    let value = tokio::time::timeout_at(deadline, async {
        let response = request
            .send()
            .await
            .map_err(|_| MachineFailure::ProvisioningUncertain)?;
        // Even a 4xx says only that this request made nothing: an earlier
        // attempt with the same key may have, so no answer here is certain.
        if !response.status().is_success() {
            return Err(MachineFailure::ProvisioningUncertain);
        }
        bounded_json(boat, response, Stage::CreateBody)
            .await
            .map_err(|_| MachineFailure::ProvisioningUncertain)
    })
    .await
    .map_err(|_| MachineFailure::ProvisioningUncertain)??;
    // A ready response is an idempotent replay of an older VM. An unknown
    // acknowledgement cannot authorize deletion of the ID it happens to name.
    let acknowledgement = match (
        value.get("ok").and_then(Value::as_bool),
        value.get("type").and_then(Value::as_str),
        value.get("status").and_then(Value::as_str),
    ) {
        (Some(true), Some("sandbox.created"), Some("provisioning")) => {
            Some(Disposition::RollbackNewVm)
        }
        (Some(true), Some("sandbox.created"), Some("ready")) => Some(Disposition::RetainTarget),
        _ => None,
    };
    let disposition = acknowledgement.unwrap_or(Disposition::RetainTarget);
    let vm = value
        .get("sandbox")
        .and_then(|s| s.get("id"))
        .and_then(Value::as_str)
        .filter(|id| vm_id(id))
        .ok_or(MachineFailure::ProvisioningUncertain)?
        .to_owned();
    let created: CreateSandboxResponse =
        match diagnostics::decode(&value, Stage::CreateDecode, SANDBOX_FIELDS) {
            Ok(created) => created,
            Err(diagnostic) => {
                diagnostic.emit(boat);
                return Ok(Created {
                    vm,
                    admission,
                    disposition,
                    refusal: Some(AdmissionRejection::new(
                        AdmissionRefusal::InvalidReceipt,
                        None,
                    )),
                });
            }
        };
    let observation = observed(&created.sandbox);
    let mut refusal = admission.check(&observation).err().map(|cause| {
        let rejection = AdmissionRejection::new(cause, Some(observation.clone()));
        if rejection.cause() == AdmissionRefusal::InvalidReceipt {
            Diagnostic::opaque(
                Stage::CreateValidation,
                "sandbox",
                "environment",
                "invalid_identity",
            )
            .emit(boat);
        }
        rejection
    });
    if acknowledgement.is_none() || !supported_state(&created.sandbox) {
        if acknowledgement.is_none() {
            let field = if value.get("ok").and_then(Value::as_bool) != Some(true) {
                "ok"
            } else if value.get("type").and_then(Value::as_str) != Some("sandbox.created") {
                "type"
            } else {
                "status"
            };
            Diagnostic::field(
                Stage::CreateValidation,
                "envelope",
                field,
                "invalid_acknowledgement",
                value.get(field),
            )
            .emit(boat);
        } else {
            Diagnostic::field(
                Stage::CreateValidation,
                "sandbox",
                "state",
                "unsupported_state",
                value.get("sandbox").and_then(|s| s.get("state")),
            )
            .emit(boat);
        }
        refusal = Some(AdmissionRejection::new(
            AdmissionRefusal::InvalidReceipt,
            Some(observation),
        ));
    }
    if refusal.is_none() {
        // Waiting for the start and checking it spend what is left of the
        // budget; running out refuses the VM, which stays named.
        let admitted = tokio::time::timeout_at(deadline, async {
            if disposition == Disposition::RollbackNewVm {
                wait_for_start(boat, &vm, deadline).await?;
            }
            let found = check(boat, spec, &vm, &admission, ttl_seconds).await?;
            if disposition == Disposition::RollbackNewVm && !found.sandbox.state.is_operable() {
                Diagnostic::opaque(Stage::InfoValidation, "sandbox", "state", "not_ready")
                    .emit(boat);
                return Err(MachineFailure::Provider);
            }
            Ok(found)
        });
        refusal = match admitted.await {
            Ok(Ok(_)) => None,
            Ok(Err(MachineFailure::AdmissionRejected(rejection))) => Some(rejection),
            Ok(Err(_)) | Err(_) => Some(AdmissionRejection::new(
                AdmissionRefusal::ReceiptUnavailable,
                None,
            )),
        };
    }
    Ok(Created {
        vm,
        admission,
        refusal,
        disposition,
    })
}

/// A fresh snapshot deployment may still be removing inherited credentials.
/// Polling grants no workspace authority: the complete strict receipt and
/// environment check still run afterwards. Replayed targets never enter here.
async fn wait_for_start(boat: &Boat, vm: &str, deadline: Instant) -> Result<(), MachineFailure> {
    tokio::time::timeout_at(deadline, async {
        loop {
            let value = info(boat, vm).await?;
            match value
                .get("sandbox")
                .and_then(|s| s.get("state"))
                .and_then(Value::as_str)
            {
                Some("init" | "provisioning" | "provisioned" | "cloning") => {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
                // The final strict reader diagnoses malformed/unsupported states
                // and rejects unsafe posture, including a wipe pending at ready.
                _ => return Ok(()),
            }
        }
    })
    .await
    .map_err(|_| {
        Diagnostic::opaque(Stage::InfoValidation, "sandbox", "state", "startup_timeout").emit(boat);
        MachineFailure::Provider
    })?
}

async fn info(boat: &Boat, vm: &str) -> Result<Value, MachineFailure> {
    let response = boat
        .request(reqwest::Method::GET, &format!("sandboxes/{vm}"))
        .send()
        .await
        .map_err(|_| MachineFailure::Provider)?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(MachineFailure::Missing);
    }
    if response.status() != reqwest::StatusCode::OK {
        return Err(MachineFailure::Provider);
    }
    bounded_json(boat, response, Stage::InfoBody).await
}

/// The SDK's Sandbox decoder ignores unknown fields. Read the actual JSON once
/// through the injected client so new credential/access fields cannot disappear.
async fn receipt(
    boat: &Boat,
    vm: &str,
    maximum_ttl: Option<u32>,
) -> Result<SandboxInfoResponse, MachineFailure> {
    let value = info(boat, vm).await?;
    let envelope = value.as_object().ok_or_else(|| {
        invalid_receipt(
            boat,
            Diagnostic::field(
                Stage::InfoValidation,
                "body",
                "envelope",
                "expected_object",
                Some(&value),
            ),
        )
    })?;
    let sandbox = value
        .get("sandbox")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            invalid_receipt(
                boat,
                Diagnostic::field(
                    Stage::InfoValidation,
                    "envelope",
                    "sandbox",
                    "expected_object",
                    value.get("sandbox"),
                ),
            )
        })?;

    let checks = [
        (
            envelope
                .keys()
                .any(|k| !matches!(k.as_str(), "ok" | "type" | "sandbox" | "giftLimitNotice")),
            "envelope",
            "unknown",
            "unknown_field",
            None,
        ),
        (
            value.get("ok") != Some(&Value::Bool(true)),
            "envelope",
            "ok",
            "invalid_value",
            value.get("ok"),
        ),
        (
            value.get("type").and_then(Value::as_str) != Some("sandbox.info"),
            "envelope",
            "type",
            "invalid_value",
            value.get("type"),
        ),
        (
            sandbox
                .keys()
                .any(|k| !SANDBOX_FIELDS.contains(&k.as_str())),
            "sandbox",
            "unknown",
            "unknown_field",
            None,
        ),
        (
            sandbox.get("id").and_then(Value::as_str) != Some(vm),
            "sandbox",
            "id",
            "identity_mismatch",
            sandbox.get("id"),
        ),
        (
            sandbox.get("access").and_then(Value::as_str) != Some("owner"),
            "sandbox",
            "access",
            "invalid_value",
            sandbox.get("access"),
        ),
        (
            sandbox.get("team") != Some(&Value::Null),
            "sandbox",
            "team",
            "expected_null",
            sandbox.get("team"),
        ),
        (
            sandbox.get("visibility") != Some(&Value::Null),
            "sandbox",
            "visibility",
            "expected_null",
            sandbox.get("visibility"),
        ),
        (
            sandbox
                .get("holdsCreatorLogins")
                .and_then(Value::as_bool)
                .is_none(),
            "sandbox",
            "holdsCreatorLogins",
            "expected_boolean",
            sandbox.get("holdsCreatorLogins"),
        ),
        (
            sandbox.get("wipePendingUntilRestart") != Some(&Value::Bool(false)),
            "sandbox",
            "wipePendingUntilRestart",
            "invalid_value",
            sandbox.get("wipePendingUntilRestart"),
        ),
        (
            sandbox.get("setupStatus") != Some(&Value::Null),
            "sandbox",
            "setupStatus",
            "expected_null",
            sandbox.get("setupStatus"),
        ),
        (
            sandbox.get("setupError") != Some(&Value::Null),
            "sandbox",
            "setupError",
            "expected_null",
            sandbox.get("setupError"),
        ),
    ];
    if let Some((_, scope, field, problem, observed)) = checks.into_iter().find(|check| check.0) {
        if field == "wipePendingUntilRestart" {
            let startup_phase = match sandbox.get("state").and_then(Value::as_str) {
                Some("ready" | "idle" | "running") => "operable",
                Some("init" | "provisioning" | "provisioned" | "cloning") => "starting",
                _ => "other",
            };
            boat.record(MachineEvent::CredentialWipePending { startup_phase });
            if observed == Some(&Value::Bool(true)) {
                Diagnostic::field(Stage::InfoValidation, scope, field, problem, observed)
                    .emit(boat);
                return Err(MachineFailure::AdmissionRejected(AdmissionRejection::new(
                    AdmissionRefusal::WipePending,
                    None,
                )));
            }
        }
        return Err(invalid_receipt(
            boat,
            Diagnostic::field(Stage::InfoValidation, scope, field, problem, observed),
        ));
    }
    // A finite choice is an upper bound. Boat may impose a shorter account
    // limit, but null, missing or longer provider TTL cannot satisfy it.
    if let Some(maximum) = maximum_ttl {
        let observed = sandbox
            .get("ttlSeconds")
            .and_then(Value::as_u64)
            .and_then(|seconds| u32::try_from(seconds).ok());
        if !observed.is_some_and(|seconds| seconds > 0 && seconds <= maximum) {
            return Err(MachineFailure::AdmissionRejected(AdmissionRejection::new(
                AdmissionRefusal::LifetimeNotEnforced,
                None,
            )));
        }
    }
    let decoded: SandboxInfoResponse =
        diagnostics::decode(&value, Stage::InfoDecode, SANDBOX_FIELDS)
            .map_err(|d| invalid_receipt(boat, d))?;
    if !supported_state(&decoded.sandbox) {
        Diagnostic::field(
            Stage::InfoValidation,
            "sandbox",
            "state",
            "unsupported_state",
            sandbox.get("state"),
        )
        .emit(boat);
        return Err(MachineFailure::AdmissionRejected(AdmissionRejection::new(
            AdmissionRefusal::InvalidReceipt,
            Some(observed(&decoded.sandbox)),
        )));
    }
    Ok(decoded)
}
fn observed(sandbox: &sandbox_client::Sandbox) -> ObservedEnvironment {
    ObservedEnvironment {
        name: sandbox.environment.clone(),
        version: sandbox.environment_version,
    }
}
pub(super) async fn check(
    boat: &Boat,
    spec: &MachineSpec,
    vm: &str,
    admitted: &EnvironmentIdentity,
    maximum_ttl: Option<u32>,
) -> Result<SandboxInfoResponse, MachineFailure> {
    let current = environment(boat, spec).await.map_err(|error| match error {
        MachineFailure::EnvironmentUnavailable => MachineFailure::AdmissionRejected(
            AdmissionRejection::new(AdmissionRefusal::EnvironmentUnavailable, None),
        ),
        other => other,
    })?;
    if &current != admitted {
        return Err(MachineFailure::AdmissionRejected(AdmissionRejection::new(
            AdmissionRefusal::EnvironmentChanged,
            None,
        )));
    }
    let found = receipt(boat, vm, maximum_ttl).await?;
    admitted.check(&observed(&found.sandbox)).map_err(|cause| {
        let rejection = AdmissionRejection::new(cause, Some(observed(&found.sandbox)));
        if rejection.cause() == AdmissionRefusal::InvalidReceipt {
            Diagnostic::opaque(
                Stage::InfoValidation,
                "sandbox",
                "environment",
                "invalid_identity",
            )
            .emit(boat);
        }
        MachineFailure::AdmissionRejected(rejection)
    })?;
    Ok(found)
}
