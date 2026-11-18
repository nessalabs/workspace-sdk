use super::*;
use crate::providers::agent37::{ApiKey, Size};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

const TEMPLATE: &str = "nessa-runtime@3";
const DIGEST: &str = "sha256:abc";
const KEY: &str = "nessa-thread-0001";
/// An ed25519 public key: the 19-byte header, then 32 key bytes.
const PUBLIC: &str =
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcH";

/// What the fake does when asked to create.
#[derive(Clone, Copy, PartialEq)]
enum OnCreate {
    Answer,
    /// Makes the instance, then answers as a gateway that lost it.
    LoseAnswer,
    /// Makes nothing and answers as a gateway that lost it.
    Fail,
    /// Makes an instance on another template.
    OtherTemplate,
}

#[derive(Default)]
struct Fake {
    instances: Vec<Value>,
    calls: Vec<String>,
    on_create: Option<OnCreate>,
    next: u32,
}
type Shared = Arc<Mutex<Fake>>;

fn instance(id: &str, status: &str, key: Option<&str>) -> Value {
    json!({
        "id": id, "status": status, "template": TEMPLATE, "template_revision": 3,
        "image_digest": DIGEST, "metadata": key.map(|key| json!({ KEY_TAG: key })),
    })
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error": {"code": "not_found", "message": "Resource not found."}})),
    )
        .into_response()
}

async fn list(State(fake): State<Shared>) -> Json<Value> {
    let mut fake = fake.lock().unwrap();
    fake.calls.push("list".into());
    Json(json!({ "data": fake.instances }))
}
async fn create(State(fake): State<Shared>, Json(body): Json<Value>) -> Response {
    let mut fake = fake.lock().unwrap();
    fake.calls.push("create".into());
    let on = fake.on_create.unwrap_or(OnCreate::Answer);
    if on == OnCreate::Fail {
        return (StatusCode::BAD_GATEWAY, "upstream gone").into_response();
    }
    fake.next += 1;
    let id = format!("inst{:06}", fake.next);
    let mut made = instance(&id, "running", body["metadata"][KEY_TAG].as_str());
    if on == OnCreate::OtherTemplate {
        made["template"] = json!("agent37-base");
    }
    assert_eq!(body["template"], TEMPLATE);
    assert_eq!(body["auto_sleep"], true);
    fake.instances.push(made.clone());
    if on == OnCreate::LoseAnswer {
        return (StatusCode::GATEWAY_TIMEOUT, "").into_response();
    }
    (StatusCode::CREATED, Json(made)).into_response()
}
async fn one(State(fake): State<Shared>, Path(id): Path<String>) -> Response {
    let mut fake = fake.lock().unwrap();
    fake.calls.push(format!("get {id}"));
    match fake.instances.iter().find(|i| i["id"] == id.as_str()) {
        Some(found) => Json(found.clone()).into_response(),
        None => not_found(),
    }
}
async fn delete(State(fake): State<Shared>, Path(id): Path<String>) -> Response {
    let mut fake = fake.lock().unwrap();
    fake.calls.push(format!("delete {id}"));
    let before = fake.instances.len();
    fake.instances.retain(|i| i["id"] != id.as_str());
    if fake.instances.len() == before {
        return not_found();
    }
    Json(json!({"id": id, "deleted": true})).into_response()
}
async fn action(
    State(fake): State<Shared>,
    Path((id, action)): Path<(String, String)>,
) -> Response {
    let mut fake = fake.lock().unwrap();
    fake.calls.push(format!("{action} {id}"));
    let Some(found) = fake.instances.iter_mut().find(|i| i["id"] == id.as_str()) else {
        return not_found();
    };
    found["status"] = json!(match action.as_str() {
        "start" => "running",
        "stop" => "stopped",
        _ => return not_found(),
    });
    Json(json!({"id": id, "status": found["status"]})).into_response()
}
async fn template(Path(name): Path<String>) -> Response {
    if name == TEMPLATE {
        Json(json!({"name": "nessa-runtime", "revision": 3})).into_response()
    } else {
        not_found()
    }
}

struct Server {
    fake: Shared,
    machines: Agent37Machines,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn start() -> Self {
        let fake = Shared::default();
        let router = Router::new()
            .route("/v1/instances", get(list).post(create))
            .route("/v1/instances/{id}", get(one).delete(delete))
            .route("/v1/instances/{id}/{action}", post(action))
            .route("/v1/templates/{name}", get(template))
            .with_state(fake.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let config = Agent37Config::new(ApiKey::new("sk_live_fixture").unwrap())
            .with_api_base(&base)
            .unwrap();
        let machines = Agent37Machines::new(config)
            .unwrap()
            .with_budget(Duration::from_secs(5));
        Self {
            fake,
            machines,
            task,
        }
    }
    fn on_create(&self, on: OnCreate) {
        self.fake.lock().unwrap().on_create = Some(on);
    }
    fn calls(&self) -> Vec<String> {
        std::mem::take(&mut self.fake.lock().unwrap().calls)
    }
    fn set(&self, id: &str, field: &str, value: Value) {
        let mut fake = self.fake.lock().unwrap();
        let found = fake.instances.iter_mut().find(|i| i["id"] == id).unwrap();
        found[field] = value;
    }
    fn add(&self, value: Value) {
        self.fake.lock().unwrap().instances.push(value);
    }
}

fn spec() -> MachineSpec {
    MachineSpec::new(
        TEMPLATE,
        Duration::from_secs(900),
        BTreeMap::from([("NESSA_THREAD_ID".into(), "0001".into())]),
    )
    .unwrap()
}

#[tokio::test]
async fn a_second_creation_with_one_key_finds_the_first_instance() {
    let server = Server::start().await;
    let first = server.machines.create(&spec(), KEY).await.unwrap();
    assert!(first.made());
    let admitted = first.admission().unwrap();
    assert_eq!((admitted.digest(), admitted.revision()), (DIGEST, Some(3)));
    assert_eq!(first.refusal(), None);
    assert_eq!(server.calls(), ["list", "create"]);
    let again = server.machines.create(&spec(), KEY).await.unwrap();
    assert!(!again.made());
    assert_eq!(again.instance(), first.instance());
    assert_eq!(server.calls(), ["list"]);
    // Another key makes another.
    let other = server
        .machines
        .create(&spec(), "nessa-thread-0002")
        .await
        .unwrap();
    assert_ne!(other.instance(), first.instance());
}

#[tokio::test]
async fn a_lost_answer_is_settled_by_looking_for_the_key() {
    let server = Server::start().await;
    server.on_create(OnCreate::LoseAnswer);
    let found = server.machines.create(&spec(), KEY).await.unwrap();
    assert!(found.made());
    assert_eq!(server.calls(), ["list", "create", "list"]);

    let server = Server::start().await;
    server.on_create(OnCreate::Fail);
    assert_eq!(
        server.machines.create(&spec(), KEY).await,
        Err(Agent37Failure::ProvisioningUncertain)
    );
}

#[tokio::test]
async fn two_instances_under_one_key_are_never_chosen_between() {
    let server = Server::start().await;
    server.add(instance("aaaaaaaaaa", "running", Some(KEY)));
    server.add(instance("bbbbbbbbbb", "stopped", Some(KEY)));
    assert_eq!(
        server.machines.create(&spec(), KEY).await,
        Err(Agent37Failure::Ambiguous)
    );
    // Asking again does not settle it.
    assert_eq!(
        server.machines.create(&spec(), KEY).await,
        Err(Agent37Failure::Ambiguous)
    );
    // One being deleted does not count.
    server.set("bbbbbbbbbb", "status", json!("deleting"));
    let found = server.machines.create(&spec(), KEY).await.unwrap();
    assert_eq!(found.instance(), "aaaaaaaaaa");
}

#[tokio::test]
async fn an_instance_that_is_not_what_was_asked_for_is_refused() {
    let server = Server::start().await;
    server.on_create(OnCreate::OtherTemplate);
    // Named, with why, so the caller can record it and delete it.
    let made = server.machines.create(&spec(), KEY).await.unwrap();
    assert_eq!(made.refusal(), Some(AdmissionRefusal::WrongTemplate));
    assert!(made.made() && made.admission().is_none());
    assert_eq!(made.observed().template, "agent37-base");
    let server = Server::start().await;
    let mut bare = instance("cccccccccc", "running", Some(KEY));
    bare["image_digest"] = Value::Null;
    server.add(bare);
    let found = server.machines.create(&spec(), KEY).await.unwrap();
    assert_eq!(
        (found.instance(), found.refusal(), found.made()),
        ("cccccccccc", Some(AdmissionRefusal::MissingDigest), false)
    );
    assert_eq!(found.observed().digest, None);
}

#[tokio::test]
async fn ready_starts_a_stopped_instance_and_refuses_a_changed_image() {
    let server = Server::start().await;
    let created = server.machines.create(&spec(), KEY).await.unwrap();
    let id = created.instance().to_owned();
    server.set(&id, "status", json!("stopped"));
    server.calls();
    server
        .machines
        .ready(&id, &created.admission().unwrap())
        .await
        .unwrap();
    assert_eq!(
        server.calls(),
        [
            format!("get {id}"),
            format!("start {id}"),
            format!("get {id}")
        ]
    );
    // Already running: one look.
    server
        .machines
        .ready(&id, &created.admission().unwrap())
        .await
        .unwrap();
    assert_eq!(server.calls(), [format!("get {id}")]);
    server.set(&id, "image_digest", json!("sha256:other"));
    assert_eq!(
        server
            .machines
            .ready(&id, &created.admission().unwrap())
            .await,
        Err(Agent37Failure::AdmissionRejected(
            AdmissionRefusal::ImageChanged
        ))
    );
    assert_eq!(
        server
            .machines
            .ready("dddddddddd", &created.admission().unwrap())
            .await,
        Err(Agent37Failure::Missing)
    );
}

#[tokio::test]
async fn ready_gives_up_within_its_budget() {
    let server = Server::start().await;
    let created = server.machines.create(&spec(), KEY).await.unwrap();
    server.set(created.instance(), "status", json!("starting"));
    let machines = server.machines.clone().with_budget(Duration::from_secs(1));
    let began = std::time::Instant::now();
    assert_eq!(
        machines
            .ready(created.instance(), &created.admission().unwrap())
            .await,
        Err(Agent37Failure::Timeout)
    );
    assert!(began.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn stop_leaves_what_already_rests_and_delete_accepts_what_is_gone() {
    let server = Server::start().await;
    let created = server.machines.create(&spec(), KEY).await.unwrap();
    let id = created.instance().to_owned();
    server.calls();
    server.machines.stop(&id).await.unwrap();
    assert_eq!(server.calls(), [format!("get {id}"), format!("stop {id}")]);
    server.set(&id, "status", json!("sleeping"));
    server.machines.stop(&id).await.unwrap();
    assert_eq!(server.calls(), [format!("get {id}")]);
    server.machines.delete(&id).await.unwrap();
    server.machines.delete(&id).await.unwrap();
    assert_eq!(
        server.calls(),
        [format!("delete {id}"), format!("delete {id}")]
    );
}

#[tokio::test]
async fn only_a_published_pinned_template_is_prepared() {
    let server = Server::start().await;
    server.machines.prepare(&spec()).await.unwrap();
    let other =
        MachineSpec::new("nessa-runtime@9", Duration::from_secs(900), BTreeMap::new()).unwrap();
    assert_eq!(
        server.machines.prepare(&other).await,
        Err(Agent37Failure::Missing)
    );
}

#[test]
fn a_spec_takes_only_a_pinned_template_and_plain_variables() {
    let idle = Duration::from_secs(900);
    assert!(MachineSpec::new(TEMPLATE, idle, BTreeMap::new()).is_ok());
    for bad in [
        "nessa-runtime",
        "@3",
        "nessa-runtime@",
        "a@b@c",
        "a b@1",
        "a/b@1",
    ] {
        assert!(
            MachineSpec::new(bad, idle, BTreeMap::new()).is_err(),
            "{bad}"
        );
    }
    for idle in [Duration::from_secs(10), Duration::from_secs(100_000)] {
        assert!(MachineSpec::new(TEMPLATE, idle, BTreeMap::new()).is_err());
    }
    for name in ["lower", "1ST", "A-B", ""] {
        let variables = BTreeMap::from([(name.to_owned(), "v".to_owned())]);
        assert!(
            MachineSpec::new(TEMPLATE, idle, variables).is_err(),
            "{name}"
        );
    }
    let sized = spec().with_size(Size {
        cpu: 4,
        memory: 8,
        disk: 20,
    });
    assert_eq!(sized.template_name(), "nessa-runtime");
}

#[test]
fn ids_and_keys_are_checked_before_anything_is_sent() {
    assert!(valid_instance("ab12cd34ef").is_ok());
    for bad in ["AB12CD34EF", "ab12cd34e", "ab12cd34ef0", "ab12cd34e/", ""] {
        assert!(valid_instance(bad).is_err(), "{bad}");
    }
    assert!(valid_key(KEY).is_ok());
    for bad in ["short", "has space here", "a/b/c/d/e/f"] {
        assert!(valid_key(bad).is_err(), "{bad}");
    }
}

#[tokio::test]
async fn neither_key_is_ever_printed() {
    let key = ApiKey::new("sk_live_secret").unwrap();
    assert!(!format!("{key:?}").contains("secret"));
    let config = Agent37Config::new(key);
    assert!(!format!("{config:?}").contains("secret"));
    assert!(ApiKey::new("sk live").is_err());
    assert!(ApiKey::new("").is_err());
    let identity = Agent37Identity::generate("printed").await.unwrap();
    let private = String::from_utf8(identity.private_key().to_vec()).unwrap();
    let body = private.lines().nth(1).unwrap();
    assert!(!format!("{identity:?}").contains(body));
    assert!(!format!("{identity:?}").contains("PRIVATE"));
    // A file that only looks like a key, or a key with another's public half.
    assert!(Agent37Identity::new(
        b"-----BEGIN OPENSSH PRIVATE KEY-----\nsecretbytes\n-----END OPENSSH PRIVATE KEY-----\n"
            .to_vec(),
        PUBLIC,
    )
    .is_err());
    assert!(Agent37Identity::new(identity.private_key().to_vec(), PUBLIC).is_err());
    assert!(Agent37Identity::new(b"not a key".to_vec(), PUBLIC).is_err());
    assert!(Agent37Identity::new(identity.private_key().to_vec(), "ssh-rsa AAAA").is_err());
    assert!(Agent37Identity::new(identity.private_key().to_vec(), "ssh-ed25519 AAAA").is_err());
}

#[tokio::test]
async fn an_instance_still_being_made_is_not_judged_yet() {
    let server = Server::start().await;
    let mut making = instance("eeeeeeeeee", "provisioning", Some(KEY));
    making["image_digest"] = Value::Null;
    server.add(making);
    assert_eq!(
        server.machines.create(&spec(), KEY).await,
        Err(Agent37Failure::ProvisioningUncertain)
    );
    server.set("eeeeeeeeee", "status", json!("running"));
    server.set("eeeeeeeeee", "image_digest", json!(DIGEST));
    let found = server.machines.create(&spec(), KEY).await.unwrap();
    assert_eq!(found.instance(), "eeeeeeeeee");
    assert!(!found.made());
}
