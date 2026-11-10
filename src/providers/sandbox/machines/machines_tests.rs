//! Hostile provider responses cross real HTTP; no live account or VM is used.
//! Ported with the tests of the application this logic came from.
use super::*;
use axum::{
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::sync::Mutex;
use tokio::task::JoinHandle;

const VM: &str = "bx_23456789";

/// Every event the machines recorded, in order.
#[derive(Default)]
struct Events(Mutex<Vec<MachineEvent>>);
impl IMachineEvents for Events {
    fn record(&self, event: MachineEvent) {
        self.0.lock().unwrap().push(event);
    }
}
impl Events {
    fn text(&self) -> String {
        format!("{:?}", self.0.lock().unwrap())
    }
}

/// Creation and info are separate messages: a cold creation is not a ready
/// GET fixture relabelled as POST. Each rejection retains its actual stage.
#[tokio::test]
async fn cold_receipt_diagnostics_distinguish_stages_without_values_or_unknown_keys() {
    for (case, stage, field, kind) in [
        (0, "create_decode", "memoryGB", "string"),
        (1, "info_validation", "setupStatus", "string"),
        (2, "info_decode", "giftLimitNotice", "object"),
        (3, "info_validation", "unknown", "not_collected"),
        (4, "create_validation", "status", "string"),
        (5, "create_validation", "state", "string"),
        (6, "info_validation", "state", "string"),
        (7, "create_validation", "environment", "not_collected"),
        (8, "info_validation", "environment", "not_collected"),
    ] {
        let server = server().await;
        let mut cold = json!({"sandbox":{
            "id":VM,"state":"provisioning","environment":"agents","environmentVersion":2
        }});
        {
            let mut state = server.state.lock().unwrap();
            match case {
                0 => cold["sandbox"]["memoryGB"] = json!("SENTINEL_CREATE_VALUE"),
                1 => state.receipt["sandbox"]["setupStatus"] = json!("SENTINEL_SETUP_VALUE"),
                2 => state.receipt["giftLimitNotice"] = json!({"value":"SENTINEL_NOTICE_VALUE"}),
                3 => {
                    state.receipt["sandbox"]["UNTRUSTED_FIELD_SECRET"] =
                        json!("SENTINEL_UNKNOWN_VALUE")
                }
                4 => state.creation_ack["status"] = json!("SENTINEL_ACK_VALUE"),
                5 => cold["sandbox"]["state"] = json!("SENTINEL_CREATE_STATE"),
                6 => state.receipt["sandbox"]["state"] = json!("SENTINEL_INFO_STATE"),
                7 => cold["sandbox"]["environment"] = json!("SENTINEL_BAD_\u{80}"),
                _ => state.receipt["sandbox"]["environment"] = json!("SENTINEL_BAD_\u{80}"),
            }
            state.creation_receipt = Some(cold);
        }
        let receipt = server.machines.create(&spec(), KEY).await.unwrap();
        assert_eq!(
            receipt.refusal.unwrap().cause(),
            AdmissionRefusal::InvalidReceipt
        );
        assert_eq!(server.state.lock().unwrap().commands, 0);
        let text = server.events.text();
        assert!(
            server.events.0.lock().unwrap().iter().any(|event| matches!(
                event,
                MachineEvent::ReceiptRejected(d)
                    if d.stage.name() == stage && d.field == field && d.observed_type == kind
            )),
            "{text}"
        );
        assert!(!text.contains("SENTINEL"), "{text}");
        assert!(!text.contains("UNTRUSTED_FIELD_SECRET"), "{text}");
        assert!(!text.contains("fixture-token"), "{text}");
    }
}

#[tokio::test]
async fn cold_creation_with_a_separate_valid_info_receipt_is_admitted() {
    let server = server().await;
    server.state.lock().unwrap().creation_receipt = Some(json!({"sandbox":{
        "id":VM,"state":"provisioning","environment":"agents","environmentVersion":2
    }}));
    let receipt = server.machines.create(&spec(), KEY).await.unwrap();
    assert!(receipt.refusal.is_none());
    assert_eq!(server.state.lock().unwrap().commands, 0);
}

/// The API's create acknowledgement precedes readiness and the wipe. No
/// command is issued while the inherited disk is still being sanitized.
#[tokio::test]
async fn cold_snapshot_waits_for_ready_then_requires_a_clean_security_receipt() {
    let server = server().await;
    {
        let mut state = server.state.lock().unwrap();
        for phase in ["provisioning", "provisioned", "cloning"] {
            let mut pending = sandbox();
            pending["sandbox"]["state"] = json!(phase);
            pending["sandbox"]["wipePendingUntilRestart"] = json!(true);
            state.startup_receipts.push_back(pending);
        }
    }
    let receipt = server.machines.create(&spec(), KEY).await.unwrap();
    assert!(receipt.refusal.is_none());
    assert_eq!(receipt.disposition, Disposition::RollbackNewVm);
    let state = server.state.lock().unwrap();
    assert_eq!(state.created, 1);
    assert_eq!(state.resumed, 0);
    assert_eq!(state.commands, 0);
    assert_eq!(state.info_reads, 5); // three pending, ready, final strict receipt
}

#[tokio::test]
async fn ready_snapshot_with_pending_wipe_is_still_refused_without_commands() {
    let server = server().await;
    server.state.lock().unwrap().receipt["sandbox"]["wipePendingUntilRestart"] = json!(true);
    let receipt = server.machines.create(&spec(), KEY).await.unwrap();
    assert_eq!(
        receipt.refusal.unwrap().cause(),
        AdmissionRefusal::WipePending
    );
    let text = server.events.text();
    assert!(
        server
            .events
            .0
            .lock()
            .unwrap()
            .contains(&MachineEvent::CredentialWipePending {
                startup_phase: "operable"
            }),
        "{text}"
    );
    assert!(!text.contains("fixture-token"));
    let state = server.state.lock().unwrap();
    assert_eq!(state.resumed, 0);
    assert_eq!(state.commands, 0);
}

#[tokio::test]
async fn cold_snapshot_startup_deadline_preserves_target_and_rollback_without_commands() {
    let mut fixture = server().await;
    // Keep the HTTP timeout longer than the startup budget, so this test
    // proves the lifecycle deadline rather than an incidental request timeout.
    fixture.machines = machines(
        &fixture.base,
        fixture.events.clone(),
        std::time::Duration::from_secs(3600),
    );
    let server = Arc::new(fixture);
    server.state.lock().unwrap().receipt["sandbox"]["state"] = json!("provisioning");
    let owner = server.clone();
    let opening = tokio::spawn(async move { owner.machines.create(&spec(), KEY).await });
    while server.state.lock().unwrap().info_reads == 0 {
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    tokio::time::pause();
    tokio::time::advance(std::time::Duration::from_secs(121)).await;
    let receipt = tokio::time::timeout(std::time::Duration::from_millis(50), opening)
        .await
        .expect("fresh startup exceeded its 120 second budget")
        .unwrap()
        .unwrap();
    assert_eq!(receipt.vm, VM);
    assert_eq!(receipt.disposition, Disposition::RollbackNewVm);
    assert_eq!(
        receipt.refusal.unwrap().cause(),
        AdmissionRefusal::ReceiptUnavailable
    );
    let state = server.state.lock().unwrap();
    assert_eq!(state.created, 1);
    assert_eq!(state.resumed, 0);
    assert_eq!(state.commands, 0);
}

#[tokio::test]
async fn fresh_snapshot_requires_operable_final_receipt_without_changing_replay_resume() {
    for state_name in [
        "error",
        "cancelled",
        "archived",
        "archiving",
        "provisioning",
    ] {
        let server = server().await;
        {
            let mut state = server.state.lock().unwrap();
            state.startup_receipts.push_back(sandbox());
            let mut final_receipt = sandbox();
            final_receipt["sandbox"]["state"] = json!(state_name);
            state.read_override = Some(final_receipt);
        }
        let receipt = server.machines.create(&spec(), KEY).await.unwrap();
        assert_eq!(receipt.vm, VM);
        assert_eq!(receipt.disposition, Disposition::RollbackNewVm);
        assert_eq!(
            receipt.refusal.unwrap().cause(),
            AdmissionRefusal::ReceiptUnavailable,
            "{state_name}"
        );
        let state = server.state.lock().unwrap();
        assert_eq!(state.created, 1);
        assert_eq!(state.resumed, 0);
        assert_eq!(state.commands, 0);
    }
}

#[tokio::test]
async fn replayed_archived_receipt_retains_existing_resume_path_without_startup_wait() {
    let server = server().await;
    {
        let mut state = server.state.lock().unwrap();
        state.creation_ack["status"] = json!("ready");
        state.receipt["sandbox"]["state"] = json!("archived");
    }
    let receipt = server.machines.create(&spec(), KEY).await.unwrap();
    assert!(receipt.refusal.is_none());
    assert_eq!(receipt.disposition, Disposition::RetainTarget);
    let state = server.state.lock().unwrap();
    assert_eq!(state.info_reads, 1);
    assert_eq!(state.resumed, 0);
    assert_eq!(state.commands, 0);
}

fn environment() -> Value {
    serde_json::from_str(include_str!("fixtures/boat-v1-environment.json")).unwrap()
}

fn sandbox() -> Value {
    let mut value: Value =
        serde_json::from_str(include_str!("fixtures/boat-v1-sandbox.json")).unwrap();
    value["sandbox"]["id"] = json!(VM);
    value["sandbox"]["state"] = json!("ready");
    value
}
struct State {
    environment: Value,
    receipt: Value,
    created: usize,
    resumed: usize,
    commands: usize,
    drift_on_create: bool,
    drift_on_resume: bool,
    creation_ack: Value,
    creation_receipt: Option<Value>,
    read_override: Option<Value>,
    startup_receipts: std::collections::VecDeque<Value>,
    info_reads: usize,
    expected_ttl: Option<u32>,
    /// Reads that still find an archiving VM archiving; the next finds it
    /// archived.
    archiving_reads: usize,
    /// The status the creation answers with.
    create_status: u16,
    /// The idempotency key a creation must carry.
    expected_key: String,
    /// Deletions asked; Boat refuses each.
    deleted: usize,
    /// The workspace's ownership marker, readable only while the VM runs.
    owner: Option<Value>,
}
struct Server {
    base: String,
    machines: BoatMachines,
    events: Arc<Events>,
    state: Arc<Mutex<State>>,
    task: JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn server() -> Server {
    let state = Arc::new(Mutex::new(State {
        environment: environment(),
        receipt: sandbox(),
        created: 0,
        resumed: 0,
        commands: 0,
        drift_on_create: false,
        drift_on_resume: false,
        creation_ack: json!({"ok":true,"type":"sandbox.created","status":"provisioning"}),
        creation_receipt: None,
        read_override: None,
        startup_receipts: Default::default(),
        info_reads: 0,
        expected_ttl: Some(3600),
        archiving_reads: 0,
        create_status: 200,
        expected_key: KEY.into(),
        deleted: 0,
        owner: None,
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api/v1", listener.local_addr().unwrap());
    let env = state.clone();
    let create = state.clone();
    let read = state.clone();
    let resume = state.clone();
    let commands = state.clone();
    let delete = state.clone();
    let files = state.clone();
    let router = Router::new()
        .route("/api/v1/me", get(|| async move { Json(json!({"ok":true,"type":"me","user":{"id":"11111111-1111-4111-8111-111111111111"}})) }))
        .route("/api/v1/environments", get(move || { let env=env.lock().unwrap().environment.clone(); async move { Json(json!({"ok":true,"type":"environments.list","environments":[env]})) } }))
        .route("/api/v1/sandboxes", post(move |headers:axum::http::HeaderMap, Json(request):Json<Value>| {
            assert_eq!(headers.get("authorization").unwrap(),"Bearer fixture-token");
            assert_eq!(headers.get("x-boat-org").unwrap(),"personal");
            let mut state=create.lock().unwrap();
            assert_eq!(headers.get("idempotency-key").unwrap(),state.expected_key.as_str());
            assert_eq!(request,json!({"type":"small","ttlSeconds":state.expected_ttl,"environment":"agents","env":{"WORKSPACE":"fixture-workspace"},"from":"bsh_fixture","snapshots":true}));
            state.created+=1; if state.drift_on_create { state.environment["passGithub"]=json!(true); } let mut body=state.creation_receipt.as_ref().unwrap_or(&state.receipt).clone(); for field in ["ok","type","status"] {body.as_object_mut().unwrap().remove(field);} for (field,value) in state.creation_ack.as_object().unwrap() {body[field.as_str()]=value.clone();} body["ttlSeconds"]=json!(state.expected_ttl); let status=axum::http::StatusCode::from_u16(state.create_status).unwrap(); async move { (status, Json(body)) } }))
        .route("/api/v1/sandboxes/{id}", get(move || { let mut state=read.lock().unwrap(); state.info_reads+=1; if state.receipt["sandbox"]["state"]==json!("archiving") { if state.archiving_reads==0 { state.receipt["sandbox"]["state"]=json!("archived"); } else { state.archiving_reads-=1; } } let body=state.startup_receipts.pop_front().unwrap_or_else(|| state.read_override.as_ref().unwrap_or(&state.receipt).clone()); async { Json(body) } }).delete(move || { let mut state=delete.lock().unwrap(); state.deleted+=1; async { axum::http::StatusCode::FORBIDDEN } }))
        .route("/api/v1/sandboxes/{id}/resume", post(move |Json(request):Json<Value>| { let mut state=resume.lock().unwrap(); assert_eq!(request["ttlSeconds"],json!(state.expected_ttl)); state.resumed+=1; state.receipt["sandbox"]["wipePendingUntilRestart"]=json!(false); if state.drift_on_resume { state.environment["passSecrets"]=json!(true); } state.receipt["sandbox"]["state"]=json!("ready"); async move { Json(json!({"ok":true,"type":"sandbox.resumed","id":VM,"status":"ready"})) } }))
        .route("/api/v1/sandboxes/{id}/files", get(move || { let state=files.lock().unwrap(); let running=state.receipt["sandbox"]["state"]==json!("ready"); let owner=state.owner.clone().filter(|_| running); async move { match owner { Some(owner) => { use base64::Engine; let content=base64::engine::general_purpose::STANDARD.encode(owner.to_string()); (axum::http::StatusCode::OK, Json(json!({"ok":true,"type":"file","content":content,"encoding":"base64"}))) } None => (axum::http::StatusCode::CONFLICT, Json(json!({"ok":false}))) } } }))
        .fallback(move || { commands.lock().unwrap().commands+=1; async { axum::http::StatusCode::FORBIDDEN } });
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let events = Arc::new(Events::default());
    let machines = machines(&base, events.clone(), std::time::Duration::from_secs(5));
    Server {
        base,
        machines,
        events,
        state,
        task,
    }
}
const KEY: &str = "creation-fixture-key";
fn spec() -> MachineSpec {
    spec_lasting(Some(3600))
}
fn spec_lasting(lifetime: Option<u32>) -> MachineSpec {
    MachineSpec::new(
        "bsh_fixture",
        "agents",
        "small",
        lifetime,
        [("WORKSPACE".into(), "fixture-workspace".into())].into(),
    )
    .unwrap()
}
fn machines(base: &str, events: Arc<Events>, timeout: std::time::Duration) -> BoatMachines {
    let mut config = sandbox_client::ClientConfig::new("fixture-token")
        .unwrap()
        .with_base_path(base)
        .unwrap();
    config.org = Some("personal".into());
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .unwrap();
    BoatMachines::new(Api::new(config).unwrap(), http, events)
}

fn account(base: &str, events: Arc<Events>) -> BoatMachines {
    let mut config = sandbox_client::ClientConfig::new("fixture-token")
        .unwrap()
        .with_base_path(base)
        .unwrap();
    config.request_timeout = std::time::Duration::from_secs(5);
    BoatMachines::for_account(Api::new(config).unwrap(), events).unwrap()
}

#[tokio::test]
async fn an_account_reads_receipts_with_its_own_client_and_follows_no_redirect() {
    let server = server().await;
    let receipt = account(&server.base, server.events.clone())
        .create(&spec(), KEY)
        .await
        .unwrap();
    assert_eq!(receipt.vm, VM);
    assert!(receipt.refusal.is_none());
    assert_eq!(server.state.lock().unwrap().created, 1);

    // A creation answered with a redirect is not followed: whatever the
    // other address would make, this one made nothing it can name.
    let target = server.base.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api/v1", listener.local_addr().unwrap());
    let redirect = Router::new().fallback(move |uri: axum::http::Uri| {
        let to = format!(
            "{}{}",
            target.trim_end_matches("/api/v1"),
            uri.path_and_query().map_or("", |p| p.as_str())
        );
        async move {
            (
                axum::http::StatusCode::TEMPORARY_REDIRECT,
                [(axum::http::header::LOCATION, to)],
            )
        }
    });
    let task = tokio::spawn(async move { axum::serve(listener, redirect).await.unwrap() });
    let redirected = account(&base, server.events.clone())
        .create(&spec(), KEY)
        .await;
    task.abort();
    assert_eq!(redirected, Err(MachineFailure::ProvisioningUncertain));
    assert_eq!(server.state.lock().unwrap().created, 1);
}

#[tokio::test]
async fn admitted_creation_sends_explicit_null_for_no_automatic_stop() {
    let server = server().await;
    {
        let mut state = server.state.lock().unwrap();
        state.expected_ttl = None;
        state.receipt["sandbox"]["ttlSeconds"] = Value::Null;
    }
    let unlimited = spec_lasting(None);
    let receipt = server.machines.create(&unlimited, KEY).await.unwrap();
    assert_eq!(receipt.vm, VM);
    assert!(receipt.refusal.is_none());
    assert_eq!(server.state.lock().unwrap().created, 1);
}

#[tokio::test]
async fn finite_creation_refuses_unenforced_provider_lifetime_before_it_is_used() {
    for observed in [Some(Value::Null), Some(json!(0)), Some(json!(14_400)), None] {
        let server = server().await;
        {
            let mut state = server.state.lock().unwrap();
            state.expected_ttl = Some(7_200);
            if let Some(value) = observed {
                state.receipt["sandbox"]["ttlSeconds"] = value;
            } else {
                state.receipt["sandbox"]
                    .as_object_mut()
                    .unwrap()
                    .remove("ttlSeconds");
            }
        }
        let finite = spec_lasting(Some(7_200));
        let receipt = server.machines.create(&finite, KEY).await.unwrap();
        assert_eq!(receipt.vm, VM);
        assert_eq!(
            receipt.refusal.as_ref().map(|rejection| rejection.cause()),
            Some(AdmissionRefusal::LifetimeNotEnforced)
        );
        let state = server.state.lock().unwrap();
        assert_eq!(state.created, 1);
        assert_eq!(state.commands, 0);
    }
}

#[tokio::test]
async fn a_shorter_provider_lifetime_satisfies_the_selected_maximum() {
    let server = server().await;
    server.state.lock().unwrap().expected_ttl = Some(7_200);
    let finite = spec_lasting(Some(7_200));
    let receipt = server.machines.create(&finite, KEY).await.unwrap();
    assert!(receipt.refusal.is_none());
    assert_eq!(server.state.lock().unwrap().commands, 0);
}

#[tokio::test]
async fn full_agent_only_posture_rejects_each_extra_credential_and_setup_path_before_create() {
    let server = server().await;
    server.machines.prepare(&spec()).await.unwrap();
    for (field, value) in [
        ("safeForThirdParties", json!(true)),
        ("passAgentsCredentials", json!(false)),
        ("passGithub", json!(true)),
        ("passSecrets", json!(true)),
        ("passSandboxCredentials", json!(true)),
        ("envContents", json!("ATTACK=1")),
        ("secretFiles", json!([{"path":".x","contents":"x"}])),
        ("selectedRepositories", json!([{"setupScript":"attack"}])),
        ("credentialProfileId", json!("unexpected")),
        ("proxyAiCredentials", json!(true)),
        ("access", json!("use")),
        ("access", json!({"restricted":true,"members":[]})),
        ("access", json!({"restricted":false,"members":["other"]})),
        (
            "effective",
            json!({"passGithub":true,"passSecrets":false,"passSandboxCredentials":false,"passAgentsCredentials":true}),
        ),
        ("futureCredentialSource", json!(true)),
        ("latestVersionNumber", json!(3)),
        (
            "latestVersionId",
            json!("33333333-3333-4333-8333-333333333333"),
        ),
    ] {
        let mut bad = environment();
        bad[field] = value;
        server.state.lock().unwrap().environment = bad;
        assert_eq!(
            server.machines.create(&spec(), KEY).await.err(),
            Some(MachineFailure::EnvironmentUnavailable),
            "{field}"
        );
    }
    for field in [
        "credentialProfileId",
        "proxyAiCredentials",
        "latestVersionNumber",
        "access",
        "effective",
    ] {
        let mut bad = environment();
        bad.as_object_mut().unwrap().remove(field);
        server.state.lock().unwrap().environment = bad;
        assert_eq!(
            server.machines.create(&spec(), KEY).await.err(),
            Some(MachineFailure::EnvironmentUnavailable),
            "missing {field}"
        );
    }
    for field in [
        "passGithub",
        "passSecrets",
        "passSandboxCredentials",
        "passAgentsCredentials",
    ] {
        let mut bad = environment();
        bad["effective"][field] = json!(!bad["effective"][field].as_bool().unwrap());
        server.state.lock().unwrap().environment = bad;
        assert_eq!(
            server.machines.create(&spec(), KEY).await.err(),
            Some(MachineFailure::EnvironmentUnavailable),
            "effective.{field}"
        );
    }
    assert_eq!(server.state.lock().unwrap().created, 0);
}

#[tokio::test]
async fn post_create_environment_drift_retains_the_real_target_but_grants_no_authority() {
    let server = server().await;
    server.state.lock().unwrap().drift_on_create = true;
    let receipt = server.machines.create(&spec(), KEY).await.unwrap();
    assert_eq!(receipt.vm, VM);
    assert_eq!(
        receipt.refusal.map(|r| r.cause()),
        Some(AdmissionRefusal::EnvironmentUnavailable)
    );
    assert_eq!(server.state.lock().unwrap().commands, 0);
}

#[tokio::test]
async fn mismatched_vm_receipts_retain_targets_without_admitting_them() {
    let server = server().await;
    for (field, value, cause) in [
        (
            "environment",
            Value::Null,
            AdmissionRefusal::MissingEnvironment,
        ),
        (
            "environment",
            json!("other"),
            AdmissionRefusal::WrongEnvironment,
        ),
        (
            "environmentVersion",
            Value::Null,
            AdmissionRefusal::MissingVersion,
        ),
        (
            "environmentVersion",
            json!(3),
            AdmissionRefusal::WrongVersion,
        ),
        (
            "environmentVersion",
            json!(4_294_967_296_u64),
            AdmissionRefusal::InvalidReceipt,
        ),
        (
            "memoryGB",
            json!("invalid"),
            AdmissionRefusal::InvalidReceipt,
        ),
        ("state", json!("unknown"), AdmissionRefusal::InvalidReceipt),
        (
            "futureCredentialSource",
            json!(true),
            AdmissionRefusal::InvalidReceipt,
        ),
        (
            "team",
            json!({"id":"other","name":"Other"}),
            AdmissionRefusal::InvalidReceipt,
        ),
        (
            "visibility",
            json!("shared"),
            AdmissionRefusal::InvalidReceipt,
        ),
        (
            "wipePendingUntilRestart",
            json!(true),
            AdmissionRefusal::WipePending,
        ),
        (
            "wipePendingUntilRestart",
            json!("yes"),
            AdmissionRefusal::InvalidReceipt,
        ),
        (
            "setupStatus",
            json!("running"),
            AdmissionRefusal::InvalidReceipt,
        ),
    ] {
        let mut body = sandbox();
        body["sandbox"][field] = value;
        server.state.lock().unwrap().receipt = body;
        let receipt = server.machines.create(&spec(), KEY).await.unwrap();
        assert_eq!(receipt.vm, VM);
        assert_eq!(receipt.refusal.map(|r| r.cause()), Some(cause), "{field}");
    }
    assert_eq!(server.state.lock().unwrap().commands, 0);
}

#[tokio::test]
async fn reconnect_refuses_changed_environment_identity_before_resume() {
    let server = server().await;
    let receipt = server.machines.create(&spec(), KEY).await.unwrap();
    assert_eq!(receipt.refusal, None);
    {
        let mut state = server.state.lock().unwrap();
        state.receipt["sandbox"]["state"] = json!("archived");
        state.environment["id"] = json!("33333333-3333-4333-8333-333333333333");
    }
    assert_eq!(
        server
            .machines
            .ready(&spec(), VM, receipt.admission())
            .await
            .err(),
        Some(MachineFailure::AdmissionRejected(AdmissionRejection::new(
            AdmissionRefusal::EnvironmentChanged,
            None
        )))
    );
    let state = server.state.lock().unwrap();
    assert_eq!(state.resumed, 0);
    assert_eq!(state.commands, 0);
}

#[tokio::test]
async fn resume_rechecks_environment_before_it_is_used() {
    let server = server().await;
    let receipt = server.machines.create(&spec(), KEY).await.unwrap();
    {
        let mut state = server.state.lock().unwrap();
        state.receipt["sandbox"]["state"] = json!("archived");
        state.drift_on_resume = true;
    }
    assert_eq!(
        server
            .machines
            .ready(&spec(), VM, receipt.admission())
            .await
            .err(),
        Some(MachineFailure::AdmissionRejected(AdmissionRejection::new(
            AdmissionRefusal::EnvironmentUnavailable,
            None
        )))
    );
    let state = server.state.lock().unwrap();
    assert_eq!(state.resumed, 1);
    assert_eq!(
        state.commands, 1,
        "its one other request stops the VM it woke"
    );
}

/// A VM still stopping from its thread's last rest is waited out, then
/// resumed, and checked again before anything runs in it.
#[tokio::test]
async fn a_vm_still_stopping_is_resumed_once_it_has_stopped() {
    let server = server().await;
    let receipt = server.machines.create(&spec(), KEY).await.unwrap();
    {
        let mut state = server.state.lock().unwrap();
        state.receipt["sandbox"]["state"] = json!("archiving");
        state.archiving_reads = 2;
        // Stops the opening after the resume, before any command.
        state.drift_on_resume = true;
    }
    assert!(matches!(
        server
            .machines
            .ready(&spec(), VM, receipt.admission())
            .await,
        Err(MachineFailure::AdmissionRejected(_))
    ));
    let state = server.state.lock().unwrap();
    assert_eq!(state.archiving_reads, 0, "it waited while the VM stopped");
    assert_eq!(state.resumed, 1, "then resumed it");
    assert_eq!(
        state.commands, 1,
        "its one other request stops the VM it woke"
    );
    assert!(server
        .events
        .0
        .lock()
        .unwrap()
        .contains(&MachineEvent::StopRequested {
            vm: VM.into(),
            cause: StopCause::NotReady
        }));
}

#[tokio::test]
async fn archived_resume_sends_explicit_null_after_lifetime_changes() {
    let server = server().await;
    let receipt = server.machines.create(&spec(), KEY).await.unwrap();
    {
        let mut state = server.state.lock().unwrap();
        state.receipt["sandbox"]["state"] = json!("archived");
        state.expected_ttl = None;
        state.drift_on_resume = true;
    }
    let unlimited = spec_lasting(None);
    assert!(matches!(
        server
            .machines
            .ready(&unlimited, VM, receipt.admission())
            .await,
        Err(MachineFailure::AdmissionRejected(_))
    ));
    assert_eq!(server.state.lock().unwrap().resumed, 1);
}

#[tokio::test]
async fn archived_resume_refuses_an_unenforced_finite_lifetime_before_it_is_used() {
    let server = server().await;
    let receipt = server.machines.create(&spec(), KEY).await.unwrap();
    {
        let mut state = server.state.lock().unwrap();
        state.receipt["sandbox"]["state"] = json!("archived");
        state.receipt["sandbox"]["ttlSeconds"] = Value::Null;
        state.expected_ttl = Some(7_200);
    }
    let finite = spec_lasting(Some(7_200));
    let result = server
        .machines
        .ready(&finite, VM, receipt.admission())
        .await;
    assert!(matches!(
        result,
        Err(MachineFailure::AdmissionRejected(rejection))
            if rejection.cause() == AdmissionRefusal::LifetimeNotEnforced
    ));
    let state = server.state.lock().unwrap();
    assert_eq!(state.resumed, 1);
    assert_eq!(
        state.commands, 1,
        "its one other request stops the VM it woke"
    );
}

#[tokio::test]
async fn an_active_vm_keeps_its_original_lifetime_after_settings_change() {
    let server = server().await;
    let receipt = server.machines.create(&spec(), KEY).await.unwrap();
    let changed = spec_lasting(Some(7_200));
    // A running VM is checked without a lifetime bound: it keeps the one it
    // was made with until its next resume.
    let result = server
        .machines
        .ready(&changed, VM, receipt.admission())
        .await;
    assert!(result.is_ok(), "{result:?}");
    let state = server.state.lock().unwrap();
    assert_eq!(state.resumed, 0);
    assert_eq!(state.commands, 0);
}

#[tokio::test]
async fn a_personal_vm_without_stored_logins_is_admitted_as_it_is() {
    let server = server().await;
    server.state.lock().unwrap().receipt["sandbox"]["holdsCreatorLogins"] = json!(false);
    let receipt = server.machines.create(&spec(), KEY).await.unwrap();
    assert_eq!(receipt.refusal, None);
    server.state.lock().unwrap().receipt["sandbox"]
        .as_object_mut()
        .unwrap()
        .remove("holdsCreatorLogins");
    let receipt = server.machines.create(&spec(), KEY).await.unwrap();
    assert_eq!(
        receipt.refusal.map(|r| r.cause()),
        Some(AdmissionRefusal::InvalidReceipt)
    );
    assert_eq!(server.state.lock().unwrap().commands, 0);
}

#[tokio::test]
async fn replayed_or_unrecognized_creation_receipts_do_not_authorize_rollback() {
    let server = server().await;
    for ack in [
        json!({"ok":true,"type":"sandbox.created","status":"ready"}),
        json!({"ok":false,"type":"sandbox.created","status":"provisioning"}),
        json!({"ok":true,"type":"unrecognized","status":"provisioning"}),
        json!({"ok":true,"type":"sandbox.created","status":"unrecognized"}),
        json!({"type":"sandbox.created","status":"provisioning"}),
        json!({"ok":true,"status":"provisioning"}),
        json!({"ok":true,"type":"sandbox.created"}),
    ] {
        {
            let mut state = server.state.lock().unwrap();
            state.creation_ack = ack;
            state.receipt["sandbox"]["environmentVersion"] = json!(3);
        }
        let receipt = server.machines.create(&spec(), KEY).await.unwrap();
        assert_eq!(receipt.vm, VM);
        assert_eq!(receipt.disposition, Disposition::RetainTarget);
        assert!(receipt.refusal.is_some());
    }
    assert_eq!(server.state.lock().unwrap().commands, 0);
}

#[tokio::test]
async fn oversized_vm_bodies_are_bounded_without_losing_a_previously_received_target() {
    let server = server().await;
    let mut large = sandbox();
    large["sandbox"]["labels"] = json!({"padding":"x".repeat(65_537)});
    server.state.lock().unwrap().receipt = large.clone();
    assert_eq!(
        server.machines.create(&spec(), KEY).await.err(),
        Some(MachineFailure::ProvisioningUncertain)
    );
    {
        let mut state = server.state.lock().unwrap();
        state.receipt = sandbox();
        state.read_override = Some(large);
    }
    let receipt = server.machines.create(&spec(), KEY).await.unwrap();
    assert_eq!(receipt.vm, VM);
    assert_eq!(
        receipt.refusal.map(|r| r.cause()),
        Some(AdmissionRefusal::InvalidReceipt)
    );
    assert_eq!(server.state.lock().unwrap().commands, 0);
}

#[tokio::test]
async fn a_stop_is_recorded_before_it_is_asked_and_once_it_is_answered() {
    let server = server().await;
    // The fixture answers no stop route, so Boat refuses it.
    assert_eq!(
        server.machines.stop(VM, StopCause::Idle).await,
        Err(MachineFailure::Provider)
    );
    assert_eq!(
        *server.events.0.lock().unwrap(),
        [
            MachineEvent::StopRequested {
                vm: VM.into(),
                cause: StopCause::Idle
            },
            MachineEvent::StopAnswered {
                vm: VM.into(),
                cause: StopCause::Idle,
                failure: Some("provider")
            },
        ]
    );
}

#[tokio::test]
async fn a_rollback_is_never_reported_as_confirmed() {
    let server = server().await;
    let created = server.machines.create(&spec(), KEY).await.unwrap();
    assert_eq!(created.disposition(), Disposition::RollbackNewVm);
    assert!(server.machines.roll_back(&created).await.is_err());
    assert_eq!(
        *server.events.0.lock().unwrap(),
        [MachineEvent::RollbackObserved {
            vm: VM.into(),
            confirmed: false
        }]
    );
}

#[tokio::test]
async fn an_archive_that_never_ends_fails_within_the_callers_budget() {
    let mut server = server().await;
    let receipt = server.machines.create(&spec(), KEY).await.unwrap();
    {
        let mut state = server.state.lock().unwrap();
        state.receipt["sandbox"]["state"] = json!("archiving");
        state.archiving_reads = usize::MAX;
    }
    server.machines = server
        .machines
        .clone()
        .with_budget(Budget::new(std::time::Duration::from_secs(2)).unwrap());
    let started = std::time::Instant::now();
    assert_eq!(
        server
            .machines
            .ready(&spec(), VM, receipt.admission())
            .await
            .err(),
        Some(MachineFailure::Provider)
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert_eq!(server.state.lock().unwrap().resumed, 0);
}

/// A sink that panics changes nothing the caller is told.
#[tokio::test]
async fn a_failing_event_sink_never_changes_an_outcome() {
    struct Panics;
    impl IMachineEvents for Panics {
        fn record(&self, _: MachineEvent) {
            panic!("sink failed");
        }
    }
    let server = server().await;
    let mut config = sandbox_client::ClientConfig::new("fixture-token")
        .unwrap()
        .with_base_path(&server.base)
        .unwrap();
    config.org = Some("personal".into());
    let machines = BoatMachines::new(
        Api::new(config).unwrap(),
        reqwest::Client::builder().no_proxy().build().unwrap(),
        Arc::new(Panics),
    );
    server.state.lock().unwrap().receipt["sandbox"]["wipePendingUntilRestart"] = json!(true);
    let receipt = machines.create(&spec(), KEY).await.unwrap();
    assert_eq!(
        receipt.refusal.unwrap().cause(),
        AdmissionRefusal::WipePending
    );
    assert_eq!(
        machines.stop(VM, StopCause::Closed).await,
        Err(MachineFailure::Provider)
    );
}

#[tokio::test]
async fn a_replayed_vm_is_never_rolled_back() {
    let server = server().await;
    server.state.lock().unwrap().creation_ack["status"] = json!("ready");
    let created = server.machines.create(&spec(), KEY).await.unwrap();
    assert_eq!(created.disposition(), Disposition::RetainTarget);
    assert_eq!(
        server.machines.roll_back(&created).await,
        Err(MachineFailure::InvalidInput)
    );
    assert!(server.events.0.lock().unwrap().is_empty());
    assert_eq!(server.state.lock().unwrap().commands, 0);
}

/// A tiny budget ends the whole readiness, whichever step it falls in; the
/// client's readiness wait alone would read a zero timeout as no deadline.
#[tokio::test]
async fn a_tiny_budget_ends_readiness_instead_of_waiting_without_end() {
    let mut server = server().await;
    let receipt = server.machines.create(&spec(), KEY).await.unwrap();
    server.state.lock().unwrap().receipt["sandbox"]["state"] = json!("archived");
    server.machines = server
        .machines
        .clone()
        .with_budget(Budget::new(std::time::Duration::from_millis(1)).unwrap());
    let ready = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        server.machines.ready(&spec(), VM, receipt.admission()),
    )
    .await
    .expect("a spent budget waited past its deadline");
    assert_eq!(ready.err(), Some(MachineFailure::Provider));
}

/// The admission checks accept only personal VMs, so a caller's other
/// scope, or none, is never used: it would make VMs that are then refused.
#[tokio::test]
async fn every_call_is_made_in_the_personal_scope() {
    let server = server().await;
    for org in [None, Some("team_other".to_owned())] {
        let mut config = sandbox_client::ClientConfig::new("fixture-token")
            .unwrap()
            .with_base_path(&server.base)
            .unwrap();
        config.org = org;
        let machines = BoatMachines::new(
            Api::new(config).unwrap(),
            reqwest::Client::builder().no_proxy().build().unwrap(),
            server.events.clone(),
        );
        // The fake server refuses a creation without `X-Boat-Org: personal`.
        let receipt = machines.create(&spec(), KEY).await.unwrap();
        assert!(receipt.refusal.is_none());
    }
    assert_eq!(server.state.lock().unwrap().created, 2);
}

#[tokio::test]
async fn a_key_or_vm_boat_could_never_use_is_refused_before_asking() {
    let server = server().await;
    for key in ["", "has space", "line\nbreak", &"k".repeat(256)] {
        assert_eq!(
            server.machines.create(&spec(), key).await.err(),
            Some(MachineFailure::InvalidInput),
            "{key:?}"
        );
    }
    let receipt = server.machines.create(&spec(), KEY).await.unwrap();
    for vm in ["", "../me", "bx_1?x", &"v".repeat(129)] {
        assert_eq!(
            server
                .machines
                .ready(&spec(), vm, receipt.admission())
                .await
                .err(),
            Some(MachineFailure::InvalidInput)
        );
        assert_eq!(
            server.machines.stop(vm, StopCause::Idle).await,
            Err(MachineFailure::InvalidInput)
        );
    }
    let state = server.state.lock().unwrap();
    assert_eq!(state.created, 1);
    assert_eq!(state.commands, 0);
    assert!(!server
        .events
        .0
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, MachineEvent::StopRequested { .. })));
}

/// No answer that fails to name a VM is certain: a refusal of a retry may
/// follow an earlier attempt with the same key that did make one.
#[tokio::test]
async fn every_answer_without_a_vm_stays_uncertain() {
    let server = server().await;
    for status in [400, 401, 403, 404, 408, 409, 422, 429, 500, 503] {
        server.state.lock().unwrap().create_status = status;
        assert_eq!(
            server.machines.create(&spec(), KEY).await.err(),
            Some(MachineFailure::ProvisioningUncertain),
            "{status}"
        );
    }
}

/// A caller that restarted rolls back the VM it saved, and still never one
/// Boat replayed.
#[tokio::test]
async fn a_saved_creation_can_still_be_rolled_back_after_a_restart() {
    let server = server().await;
    let created = server.machines.create(&spec(), KEY).await.unwrap();
    let saved = Created::restore(
        created.vm().into(),
        created.admission().clone(),
        Disposition::RollbackNewVm,
    )
    .unwrap();
    assert_eq!(
        server.machines.roll_back(&saved).await,
        Err(MachineFailure::Provider)
    );
    let replayed = Created::restore(
        VM.into(),
        created.admission().clone(),
        Disposition::RetainTarget,
    )
    .unwrap();
    assert_eq!(
        server.machines.roll_back(&replayed).await,
        Err(MachineFailure::InvalidInput)
    );
    assert_eq!(
        Created::restore(
            "../x".into(),
            created.admission().clone(),
            Disposition::RollbackNewVm
        )
        .err(),
        Some(MachineFailure::InvalidInput)
    );
}

/// The provider over these machines, as an application uses it.
mod provider {
    use super::*;
    use crate::{
        domain::{
            CreateRequest, MachineState, ProvisionKey, RefusalCleanup, RefusalKind, WorkspaceId,
        },
        infrastructure::MemoryStateRepository,
        providers::{SandboxCreateOptions, SandboxProvider},
        Error, IProvider, WorkspaceSdk,
    };

    fn options() -> SandboxCreateOptions {
        let mut options = SandboxCreateOptions::new("/work");
        options.image = Some("bsh_fixture".into());
        options.environment = Some("agents".into());
        options.sandbox_type = Some("small".into());
        options.variables = [("WORKSPACE".into(), "fixture-workspace".into())].into();
        options
    }
    fn request(options: &SandboxCreateOptions) -> CreateRequest {
        CreateRequest::with_options(WorkspaceId::new("fixture").unwrap(), options).unwrap()
    }
    fn provider(server: &Server) -> SandboxProvider {
        server.state.lock().unwrap().expected_key =
            ProvisionKey::for_workspace(&WorkspaceId::new("fixture").unwrap())
                .as_str()
                .into();
        SandboxProvider::from_machines(server.machines.clone())
    }
    async fn refusal(server: &Server) -> crate::domain::Refusal {
        match provider(server).create(request(&options())).await {
            Err(Error::Refused(refusal)) => *refusal,
            Err(other) => panic!("not refused: {other}"),
            Ok(_) => panic!("not refused"),
        }
    }

    #[test]
    fn a_machine_needs_an_image_environment_and_type_and_nothing_else_takes_them() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = runtime.block_on(server());
        let with = provider(&server);
        assert!(with.check_create(&request(&options())).is_ok());
        for strip in 0..3 {
            let mut partial = options();
            match strip {
                0 => partial.image = None,
                1 => partial.environment = None,
                _ => partial.sandbox_type = None,
            }
            assert!(matches!(
                with.check_create(&request(&partial)),
                Err(Error::InvalidInput(_))
            ));
        }
        let mut unsafe_name = options();
        unsafe_name.image = Some("../x".into());
        assert!(matches!(
            with.check_create(&request(&unsafe_name)),
            Err(Error::InvalidInput(_))
        ));
        let without = SandboxProvider::new(
            Api::new(sandbox_client::ClientConfig::new("fixture-only").unwrap()).unwrap(),
        );
        assert!(matches!(
            without.check_create(&request(&options())),
            Err(Error::InvalidInput(_))
        ));
        let mut plain = SandboxCreateOptions::new("/work");
        plain.sandbox_type = Some("small".into());
        assert!(without.check_create(&request(&plain)).is_ok());
        // An option left out keeps the request it was before.
        assert_eq!(
            serde_json::to_value(&plain).unwrap(),
            json!({"root":"/work","sandbox_type":"small","ttl_seconds":3600,"environment":null})
        );
    }

    #[tokio::test]
    async fn a_machine_in_another_environment_is_refused_with_why_and_rolled_back() {
        let server = server().await;
        {
            let mut state = server.state.lock().unwrap();
            state.receipt["sandbox"]["environment"] = json!("elsewhere");
        }
        let refusal = refusal(&server).await;
        assert_eq!(refusal.kind, RefusalKind::EnvironmentMismatch);
        assert_eq!(refusal.target.as_deref(), Some(VM));
        // Boat never confirms a deletion.
        assert_eq!(refusal.cleanup, RefusalCleanup::Unknown);
        assert!(
            refusal.message.contains("another environment")
                && refusal.message.contains("\"elsewhere\""),
            "{}",
            refusal.message
        );
        let admitted = refusal.admission.unwrap();
        assert_eq!(admitted.name, "agents");
        assert_eq!(admitted.version, "2");
        let state = server.state.lock().unwrap();
        assert_eq!((state.created, state.deleted, state.commands), (1, 1, 0));
    }

    /// A wipe may finish once the machine restarts, so it is no outcome:
    /// the machine is stopped, not deleted, and the record stays pending so
    /// the same key checks the same machine again.
    #[tokio::test]
    async fn a_pending_wipe_stops_the_machine_and_leaves_the_record_pending() {
        let server = server().await;
        server.state.lock().unwrap().receipt["sandbox"]["wipePendingUntilRestart"] = json!(true);
        let provider = Arc::new(provider(&server));
        let id = provider.id();
        let sdk = WorkspaceSdk::builder(Arc::new(MemoryStateRepository::default()))
            .provider(provider)
            .unwrap()
            .build()
            .unwrap();
        match sdk.create(&id, request(&options())).await {
            Err(Error::Provider { message, .. }) => {
                assert!(message.contains("credentials from its image"), "{message}")
            }
            _ => panic!("not a retryable failure"),
        }
        let record = sdk
            .record(&WorkspaceId::new("fixture").unwrap())
            .unwrap()
            .unwrap();
        assert!(matches!(record.state, MachineState::Provisioning { .. }));
        let state = server.state.lock().unwrap();
        // The one other request is the stop.
        assert_eq!((state.deleted, state.commands), (0, 1));
    }

    /// A replayed machine that `ready` resumed and then refused is stopped
    /// again, never deleted.
    #[tokio::test]
    async fn a_replayed_machine_refused_after_resuming_is_stopped_and_kept() {
        let server = server().await;
        {
            let mut state = server.state.lock().unwrap();
            state.creation_ack["status"] = json!("ready");
            state.receipt["sandbox"]["state"] = json!("archived");
            state.drift_on_resume = true;
        }
        let refusal = refusal(&server).await;
        assert_eq!(refusal.kind, RefusalKind::NotAdmitted);
        assert_eq!(refusal.cleanup, RefusalCleanup::Kept);
        let state = server.state.lock().unwrap();
        assert_eq!((state.resumed, state.deleted, state.commands), (1, 0, 1));
    }

    #[tokio::test]
    async fn a_replayed_machine_that_is_refused_is_never_deleted() {
        let server = server().await;
        {
            let mut state = server.state.lock().unwrap();
            state.creation_ack["status"] = json!("ready");
            state.receipt["sandbox"]["environmentVersion"] = json!(9);
        }
        let refusal = refusal(&server).await;
        assert_eq!(refusal.kind, RefusalKind::EnvironmentMismatch);
        assert_eq!(refusal.cleanup, RefusalCleanup::Kept);
        assert!(refusal.message.contains("version 9"), "{}", refusal.message);
        let state = server.state.lock().unwrap();
        // Found running, so left as it was: it may be in use elsewhere.
        assert_eq!((state.deleted, state.commands), (0, 0));
    }

    #[tokio::test]
    async fn an_unusable_receipt_is_refused_as_not_admitted() {
        let server = server().await;
        server.state.lock().unwrap().receipt["sandbox"]["setupStatus"] = json!("running");
        assert_eq!(refusal(&server).await.kind, RefusalKind::NotAdmitted);
    }

    /// The SDK saves the refusal, so asking again answers it without making
    /// another machine.
    #[tokio::test]
    async fn the_sdk_keeps_a_refusal_and_never_asks_boat_again() {
        let server = server().await;
        server.state.lock().unwrap().receipt["sandbox"]["environment"] = json!("elsewhere");
        let provider = Arc::new(provider(&server));
        let id = provider.id();
        let sdk = WorkspaceSdk::builder(Arc::new(MemoryStateRepository::default()))
            .provider(provider)
            .unwrap()
            .build()
            .unwrap();
        for _ in 0..2 {
            match sdk.create(&id, request(&options())).await {
                Err(Error::Refused(refusal)) => {
                    assert_eq!(refusal.kind, RefusalKind::EnvironmentMismatch)
                }
                _ => panic!("not refused"),
            }
        }
        let record = sdk
            .record(&WorkspaceId::new("fixture").unwrap())
            .unwrap()
            .unwrap();
        assert!(matches!(record.state, MachineState::Refused { .. }));
        assert_eq!(server.state.lock().unwrap().created, 1);
    }

    /// Boat failing before it names a machine leaves the record pending, so
    /// the same key finishes it.
    #[tokio::test]
    async fn an_uncertain_creation_stays_pending_for_the_same_key() {
        let server = server().await;
        server.state.lock().unwrap().create_status = 500;
        let provider = Arc::new(provider(&server));
        let id = provider.id();
        let sdk = WorkspaceSdk::builder(Arc::new(MemoryStateRepository::default()))
            .provider(provider)
            .unwrap()
            .build()
            .unwrap();
        assert!(matches!(
            sdk.create(&id, request(&options())).await,
            Err(Error::Provider { .. })
        ));
        let record = sdk
            .record(&WorkspaceId::new("fixture").unwrap())
            .unwrap()
            .unwrap();
        assert!(matches!(record.state, MachineState::Provisioning { .. }));
    }

    fn key() -> ProvisionKey {
        ProvisionKey::for_workspace(&WorkspaceId::new("fixture").unwrap())
    }

    /// An admitted machine is named with what it was admitted under, which
    /// the SDK saves in the Ready record.
    #[tokio::test]
    async fn an_admitted_machine_carries_its_admission() {
        let server = server().await;
        let provider = provider(&server);
        let machines = server.machines.clone();
        let (vm, machine) = provider
            .create_machine(&machines, &options(), &key())
            .await
            .unwrap();
        let admitted = machine.admission();
        assert_eq!(vm, VM);
        assert_eq!(
            admitted.environment_id,
            "11111111-1111-4111-8111-111111111111"
        );
        assert_eq!(admitted.version_id, "22222222-2222-4222-8222-222222222222");
        assert_eq!(
            (admitted.name.as_str(), admitted.version.as_str()),
            ("agents", "2")
        );
        let state = server.state.lock().unwrap();
        assert_eq!((state.created, state.deleted, state.commands), (1, 0, 0));
    }

    /// A check that may pass later is asked again with the same key: Boat
    /// replays the same machine, and once its wipe is done it is admitted.
    #[tokio::test]
    async fn a_retry_after_a_pending_wipe_admits_the_same_machine() {
        let server = server().await;
        let provider = provider(&server);
        let machines = server.machines.clone();
        server.state.lock().unwrap().receipt["sandbox"]["wipePendingUntilRestart"] = json!(true);
        assert!(matches!(
            provider.create_machine(&machines, &options(), &key()).await,
            Err(Error::Provider { .. })
        ));
        assert!(server
            .events
            .0
            .lock()
            .unwrap()
            .contains(&MachineEvent::StopRequested {
                vm: VM.into(),
                cause: StopCause::NotAdmitted
            }));
        {
            // The stop archived it; the wipe stays pending until a restart.
            let mut state = server.state.lock().unwrap();
            state.receipt["sandbox"]["state"] = json!("archived");
            state.creation_ack["status"] = json!("ready");
        }
        let (vm, _) = provider
            .create_machine(&machines, &options(), &key())
            .await
            .unwrap();
        assert_eq!(vm, VM);
        let state = server.state.lock().unwrap();
        assert_eq!((state.created, state.deleted, state.resumed), (2, 0, 1));
    }

    /// A replayed machine whose check may pass later is left as found: it
    /// may be another caller's, in use.
    #[tokio::test]
    async fn a_replayed_machine_with_a_pending_wipe_is_left_running() {
        let server = server().await;
        {
            let mut state = server.state.lock().unwrap();
            state.creation_ack["status"] = json!("ready");
            state.receipt["sandbox"]["wipePendingUntilRestart"] = json!(true);
        }
        let machines = server.machines.clone();
        assert!(provider(&server)
            .create_machine(&machines, &options(), &key())
            .await
            .is_err());
        let state = server.state.lock().unwrap();
        assert_eq!((state.deleted, state.commands), (0, 0));
    }

    /// A machine target as a reconnect builds it: its ownership not yet
    /// verified.
    fn woken(
        server: &Server,
        machine: crate::providers::sandbox::locator::Machine,
    ) -> crate::providers::sandbox::target::SandboxTarget {
        crate::providers::sandbox::target::SandboxTarget {
            api: server.machines.api().clone(),
            supervisor: crate::providers::SupervisorDeployment::default(),
            locator: crate::providers::managed::Locator {
                root: "/work/fixture/project".into(),
                control: "/work/fixture/control".into(),
                ownership: crate::domain::Ownership::SdkOwned,
                target: VM.into(),
            },
            workspace_id: WorkspaceId::new("fixture").unwrap(),
            verified_owned: std::sync::atomic::AtomicBool::new(false),
            wake: std::time::Duration::from_secs(5),
            admitted_now: std::sync::atomic::AtomicBool::new(false),
            machine: Some((server.machines.clone(), machine)),
        }
    }
    async fn made(server: &Server) -> crate::providers::sandbox::locator::Machine {
        let machines = server.machines.clone();
        provider(server)
            .create_machine(&machines, &options(), &key())
            .await
            .unwrap()
            .1
    }

    /// A stopped machine wakes through `ready`: resumed with its saved
    /// lifetime and checked again before anything runs in it.
    #[tokio::test]
    async fn a_stopped_machine_is_checked_again_on_waking() {
        use crate::providers::managed::ITarget;
        let server = server().await;
        let machine = made(&server).await;
        {
            let mut state = server.state.lock().unwrap();
            state.receipt["sandbox"]["state"] = json!("archived");
            state.drift_on_resume = true;
        }
        match woken(&server, machine).ensure().await {
            Err(Error::Refused(refusal)) => {
                assert_eq!(refusal.kind, RefusalKind::NotAdmitted);
                assert_eq!(refusal.cleanup, RefusalCleanup::Kept);
                assert_eq!(refusal.target.as_deref(), Some(VM));
            }
            Err(other) => panic!("not refused: {other}"),
            Ok(()) => panic!("not refused"),
        }
        let state = server.state.lock().unwrap();
        // Resumed (the fake checks its lifetime), then stopped again.
        assert_eq!((state.resumed, state.commands), (1, 1));
    }

    /// An environment changed since admission refuses the machine before
    /// it is woken.
    #[tokio::test]
    async fn a_machine_whose_environment_changed_is_not_woken() {
        use crate::providers::managed::ITarget;
        let server = server().await;
        let machine = made(&server).await;
        {
            let mut state = server.state.lock().unwrap();
            state.receipt["sandbox"]["state"] = json!("archived");
            state.environment["id"] = json!("33333333-3333-4333-8333-333333333333");
        }
        let woke = woken(&server, machine).ensure().await;
        assert!(
            matches!(&woke, Err(Error::Refused(r)) if r.kind == RefusalKind::EnvironmentMismatch),
            "{woke:?}"
        );
        assert_eq!(server.state.lock().unwrap().resumed, 0);
    }

    /// A machine's reference names it, and only a provider with machines
    /// reconnects to it.
    #[tokio::test]
    async fn only_a_provider_with_machines_reconnects_to_a_machine() {
        let server = server().await;
        let machine = made(&server).await;
        let locator = crate::providers::managed::Locator {
            root: "/work/fixture/project".into(),
            control: "/work/fixture/control".into(),
            ownership: crate::domain::Ownership::SdkOwned,
            target: VM.into(),
        };
        let with = provider(&server);
        let reference = crate::domain::WorkspaceRef {
            version: 1,
            id: WorkspaceId::new("fixture").unwrap(),
            provider: with.id(),
            locator: crate::providers::sandbox::locator::encode(&locator, Some(&machine)).unwrap(),
        };
        let backend = with.connect(&reference).await.unwrap();
        assert_eq!(backend.descriptor().admission, Some(machine.admission()));
        let without = SandboxProvider::new(
            Api::new(sandbox_client::ClientConfig::new("fixture-only").unwrap()).unwrap(),
        );
        assert!(matches!(
            without.connect(&reference).await,
            Err(Error::InvalidInput(_))
        ));
    }

    /// After a restart a stopped machine's marker cannot be read, so it is
    /// woken and checked first, and its ownership verified once it runs.
    /// (The fake cannot host a supervisor, so installing one fails after.)
    #[tokio::test]
    async fn a_stopped_machine_reconnected_after_a_restart_wakes_then_proves_ownership() {
        use crate::providers::managed::ITarget;
        let server = server().await;
        let machine = made(&server).await;
        {
            let mut state = server.state.lock().unwrap();
            state.receipt["sandbox"]["state"] = json!("archived");
            state.owner = Some(json!({"version":1,"workspace_id":"fixture",
                "root":"/work/fixture/project","control":"/work/fixture/control",
                "ownership":"sdk_owned"}));
        }
        let target = woken(&server, machine);
        let woke = target.ensure().await;
        assert!(
            !matches!(woke, Err(Error::Refused(_)) | Err(Error::State(_))),
            "{woke:?}"
        );
        assert!(target
            .verified_owned
            .load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(server.state.lock().unwrap().resumed, 1);
    }

    /// A machine that is not this workspace's is never used once woken.
    #[tokio::test]
    async fn a_woken_machine_with_another_owner_is_not_used() {
        use crate::providers::managed::ITarget;
        let server = server().await;
        let machine = made(&server).await;
        server.state.lock().unwrap().owner = Some(json!({"version":1,
            "workspace_id":"someone-else","root":"/work/fixture/project",
            "control":"/work/fixture/control","ownership":"sdk_owned"}));
        assert!(matches!(
            woken(&server, machine).ensure().await,
            Err(Error::Conflict(_))
        ));
    }

    /// A provider with machines made every workspace it owns as one, so a
    /// reference of its own that names none is refused.
    #[tokio::test]
    async fn a_provider_with_machines_refuses_its_own_reference_without_a_machine() {
        let server = server().await;
        let with = provider(&server);
        let reference = |ownership| crate::domain::WorkspaceRef {
            version: 1,
            id: WorkspaceId::new("fixture").unwrap(),
            provider: with.id(),
            locator: crate::providers::sandbox::locator::encode(
                &crate::providers::managed::Locator {
                    root: "/work/fixture/project".into(),
                    control: "/work/fixture/control".into(),
                    ownership,
                    target: VM.into(),
                },
                None,
            )
            .unwrap(),
        };
        assert!(matches!(
            with.connect(&reference(crate::domain::Ownership::SdkOwned))
                .await,
            Err(Error::InvalidInput(_))
        ));
        assert!(with
            .connect(&reference(crate::domain::Ownership::Attached))
            .await
            .is_ok());
    }
}
