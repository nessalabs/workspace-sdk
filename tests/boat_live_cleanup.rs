//! Loopback protocol fixtures establish orchestration and recovery, not live Boat behavior.
#![cfg(all(feature = "boat", unix))]
#[allow(dead_code)] // Shared helper also exposes live-harness workspace recording.
#[path = "support/boat_live_fixture.rs"]
mod boat_live_fixture;

use boat_live_fixture::{Budgets, CreationState, FixtureLedger, FixtureRecord};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use tempfile::TempDir;
use workspace_sdk::{
    boat_client::{Api, ClientConfig},
    infrastructure::MemoryStateRepository,
    providers::{SandboxOpenOptions, SandboxProvider},
    OpenRequest, ProviderId, WorkspaceId, WorkspaceSdk,
};

const FIRST: &str = "bx_23456789";
const SECOND: &str = "bx_abcdefgh";

#[derive(Clone, Debug)]
struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Value,
}
struct Response(u16, Value);
type Handler = dyn Fn(&Request) -> Option<Response> + Send + Sync;

/// One connection per response bounds server ownership and makes lost acknowledgments explicit.
struct FakeBoat {
    address: String,
    requests: Arc<Mutex<Vec<Request>>>,
    stopped: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl FakeBoat {
    fn start(handler: impl Fn(&Request) -> Option<Response> + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stopped = Arc::new(AtomicBool::new(false));
        let recorded = requests.clone();
        let stop = stopped.clone();
        let handler: Arc<Handler> = Arc::new(handler);
        let worker = thread::spawn(move || {
            while !stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // Accepted sockets inherit nonblocking mode on macOS.
                        stream.set_nonblocking(false).unwrap();
                        let Some(request) = read_request(&mut stream) else {
                            continue;
                        };
                        recorded.lock().unwrap().push(request.clone());
                        if let Some(Response(status, body)) = handler(&request) {
                            let body = serde_json::to_vec(&body).unwrap();
                            let header = format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                            let _ = stream.write_all(header.as_bytes());
                            let _ = stream.write_all(&body);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1))
                    }
                    Err(error) => panic!("fixture accept failed: {error}"),
                }
            }
        });
        Self {
            address,
            requests,
            stopped,
            worker: Some(worker),
        }
    }
    fn api(&self) -> Api {
        Api::new(
            ClientConfig::new("fixture-only")
                .unwrap()
                .with_base_path(format!("http://{}/api/v1", self.address))
                .unwrap(),
        )
        .unwrap()
    }
    fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}
impl Drop for FakeBoat {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            if let Err(panic) = worker.join() {
                if !thread::panicking() {
                    std::panic::resume_unwind(panic);
                }
            }
        }
    }
}
fn read_request_chunk(stream: &mut TcpStream, buffer: &mut [u8]) -> Option<usize> {
    match stream.read(buffer) {
        Ok(0) => None,
        Ok(count) => Some(count),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::TimedOut
            ) =>
        {
            None
        }
        Err(error) => panic!("fixture request read failed: {error}"),
    }
}
fn read_request(stream: &mut TcpStream) -> Option<Request> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    let header_end = loop {
        let count = read_request_chunk(stream, &mut buffer)?;
        bytes.extend_from_slice(&buffer[..count]);
        assert!(bytes.len() <= 64 * 1024, "fixture request exceeds bound");
        if let Some(index) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let header = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
    let mut lines = header.lines();
    let mut start = lines.next().unwrap().split_whitespace();
    let method = start.next().unwrap().to_owned();
    let path = start.next().unwrap().to_owned();
    let headers: HashMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length = headers
        .get("content-length")
        .map(|value| value.parse::<usize>().unwrap())
        .unwrap_or(0);
    assert!(length <= 64 * 1024);
    while bytes.len() < header_end + length {
        let count = read_request_chunk(stream, &mut buffer)?;
        bytes.extend_from_slice(&buffer[..count]);
    }
    let body = if length == 0 {
        Value::Null
    } else {
        serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap()
    };
    Some(Request {
        method,
        path,
        headers,
        body,
    })
}
fn budgets() -> Budgets {
    // Allow loaded CI to schedule loopback requests and fsync the ledger. The
    // former 35 ms cleanup window was never a performance contract.
    Budgets {
        request: Duration::from_millis(500),
        cleanup: Duration::from_secs(1),
        poll: Duration::from_millis(25),
    }
}
fn sandbox(id: &str, state: &str) -> Value {
    json!({"id":id,"name":"fixture","state":state,"desktopAvailable":false,"snapshotAvailable":false})
}
fn created(id: &str) -> Response {
    Response(
        202,
        json!({"ok":true,"type":"sandbox.created","sandbox":sandbox(id,"ready")}),
    )
}
fn operation(id: &str, target: &str, status: &str, kind: &str) -> Response {
    Response(
        200,
        json!({"ok":true,"operation":{"id":id,"kind":kind,"targetId":target,"reason":"fixture","status":status,"attemptCount":1,"requestedAt":"2026-10-17T00:00:00Z"}}),
    )
}
fn default_response(request: &Request) -> Response {
    let target = request
        .path
        .trim_start_matches("/api/v1/sandboxes/")
        .trim_end_matches("/stop");
    if request.method == "POST" && request.path.ends_with("/stop") {
        Response(200, json!({"ok":true,"id":target,"status":"archived"}))
    } else if request.method == "GET" && request.path.starts_with("/api/v1/sandboxes/") {
        Response(200, json!({"ok":true,"sandbox":sandbox(target,"archived")}))
    } else {
        Response(503, json!({"ok":false,"error":"fixture unavailable"}))
    }
}
fn ledger() -> (TempDir, std::path::PathBuf, FixtureLedger) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("fixtures.json");
    let ledger = FixtureLedger::create(&path).unwrap();
    (dir, path, ledger)
}
fn persisted_fixtures(path: &std::path::Path) -> Vec<FixtureRecord> {
    let data: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    serde_json::from_value(data["fixtures"].clone()).unwrap()
}
fn reserve(ledger: &mut FixtureLedger, name: &str) -> usize {
    ledger
        .reserve(WorkspaceId::new(name).unwrap(), 300)
        .unwrap()
}

#[tokio::test]
async fn raw_target_is_durable_before_failed_sdk_bootstrap_and_reopened_cleanup() {
    let (_dir, path, mut ledger) = ledger();
    let index = reserve(&mut ledger, "bootstrap-failure");
    let observed_path = path.clone();
    let deleted = Arc::new(AtomicBool::new(false));
    let server = FakeBoat::start(move |request| {
        if request.method == "POST" && request.path == "/api/v1/sandboxes" {
            let durable = persisted_fixtures(&observed_path);
            let fixture = &durable[index];
            assert_eq!(fixture.creation, CreationState::Uncertain);
            assert_eq!(fixture.ttl_seconds, 300);
            assert_eq!(
                request.headers["idempotency-key"],
                fixture.idempotency_key.to_string()
            );
            assert_eq!(request.body, json!({"ttlSeconds":300,"noEnv":true}));
            Some(created(FIRST))
        } else if request.method == "DELETE" {
            deleted.store(true, Ordering::Release);
            assert_eq!(request.headers["x-ascii-confirm-delete"], FIRST);
            Some(operation("delete-first", FIRST, "completed", "sandbox"))
        } else if request.method == "GET" && request.path == format!("/api/v1/sandboxes/{FIRST}") {
            Some(Response(
                200,
                json!({"ok":true,"sandbox":sandbox(FIRST,if deleted.load(Ordering::Acquire) { "archived" } else { "ready" })}),
            ))
        } else {
            Some(default_response(request))
        }
    });
    let api = server.api();
    let target = ledger.create_target(&api, index, budgets()).await.unwrap();
    drop(ledger);
    let durable = FixtureLedger::load(&path).unwrap();
    assert_eq!(durable.fixtures()[index].target_id.as_deref(), Some(FIRST));
    assert!(durable.fixtures()[index].workspace.is_none());
    let sdk = WorkspaceSdk::builder(Arc::new(MemoryStateRepository::default()))
        .provider(Arc::new(SandboxProvider::new(api.clone())))
        .unwrap()
        .build()
        .unwrap();
    let mut options = SandboxOpenOptions::new("/fixture");
    options.sandbox_id = Some(target);
    let bootstrap = tokio::time::timeout(
        Duration::from_secs(2),
        sdk.open(
            &ProviderId::new("boat").unwrap(),
            OpenRequest::new(WorkspaceId::new("bootstrap-failure").unwrap(), &options).unwrap(),
        ),
    )
    .await
    .expect("SDK bootstrap must stay bounded");
    assert!(
        bootstrap.is_err(),
        "fixture must fail actual SDK attachment/bootstrap"
    );
    assert!(
        server
            .requests()
            .iter()
            .any(|request| request.method == "POST"
                && request.path == format!("/api/v1/sandboxes/{FIRST}/commands")),
        "real SDK must reach native bootstrap before injected failure"
    );
    drop(durable);
    let mut reopened = FixtureLedger::load(&path).unwrap();
    reopened.cleanup_all(&api, budgets()).await.unwrap();
    assert!(persisted_fixtures(&path)[index].cleanup.confirmed);
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|r| r.method == "POST" && r.path == "/api/v1/sandboxes")
            .count(),
        1
    );
    assert!(!fs::read_to_string(path).unwrap().contains("fixture-only"));
}

#[tokio::test]
async fn lost_create_acknowledgment_retains_intent_and_reuses_exact_key_and_body() {
    let (_dir, path, mut ledger) = ledger();
    let index = reserve(&mut ledger, "lost-ack");
    let creates = Arc::new(Mutex::new(Vec::<Request>::new()));
    let observed = creates.clone();
    let server = FakeBoat::start(move |request| {
        if request.method == "POST" && request.path == "/api/v1/sandboxes" {
            let mut creates = observed.lock().unwrap();
            creates.push(request.clone());
            if creates.len() == 1 {
                None
            } else {
                Some(created(FIRST))
            }
        } else if request.method == "DELETE" {
            Some(operation("delete-first", FIRST, "completed", "sandbox"))
        } else {
            Some(default_response(request))
        }
    });
    let api = server.api();
    assert!(ledger.create_target(&api, index, budgets()).await.is_err());
    drop(ledger);
    let mut reopened = FixtureLedger::load(&path).unwrap();
    assert_eq!(
        reopened.fixtures()[index].creation,
        CreationState::Uncertain
    );
    assert!(reopened.fixtures()[index].target_id.is_none());
    assert!(reopened.cleanup_all(&api, budgets()).await.is_err());
    assert!(!reopened.fixtures()[index].cleanup.confirmed);
    assert_eq!(
        reopened
            .create_target(&api, index, budgets())
            .await
            .unwrap(),
        FIRST
    );
    drop(reopened);
    let mut reopened = FixtureLedger::load(&path).unwrap();
    assert_eq!(
        reopened
            .create_target(&api, index, budgets())
            .await
            .unwrap(),
        FIRST
    );
    {
        let creates = creates.lock().unwrap();
        assert_eq!(
            creates.len(),
            2,
            "known target must not create a third resource"
        );
        assert_eq!(
            creates[0].headers["idempotency-key"],
            creates[1].headers["idempotency-key"]
        );
        assert_eq!(creates[0].body, creates[1].body);
    }
    reopened.cleanup_all(&api, budgets()).await.unwrap();
}

#[derive(Clone, Copy, Debug)]
enum Failure {
    Pending,
    Accepted,
    Blocked,
    Missing,
    MissingTarget,
    OperableCompleted,
    WrongTarget,
    WrongKind,
    WrongOperation,
}
#[tokio::test]
async fn only_matching_completed_deletion_confirms_cleanup() {
    for failure in [
        Failure::Pending,
        Failure::Accepted,
        Failure::Blocked,
        Failure::Missing,
        Failure::MissingTarget,
        Failure::OperableCompleted,
        Failure::WrongTarget,
        Failure::WrongKind,
        Failure::WrongOperation,
    ] {
        let (_dir, path, mut ledger) = ledger();
        let index = reserve(&mut ledger, "incomplete-delete");
        let server = FakeBoat::start(move |request| {
            if request.method == "POST" && request.path == "/api/v1/sandboxes" {
                return Some(created(FIRST));
            }
            if request.method == "DELETE"
                || request.path.starts_with("/api/v1/deletion-operations/")
            {
                return Some(match failure {
                    Failure::Accepted => {
                        let Response(_, body) =
                            operation("delete-first", FIRST, "accepted", "sandbox");
                        Response(202, body)
                    }
                    Failure::Missing => Response(404, json!({"ok":false,"error":"not found"})),
                    Failure::OperableCompleted => {
                        operation("delete-first", FIRST, "completed", "sandbox")
                    }
                    Failure::WrongTarget => {
                        operation("delete-first", SECOND, "completed", "sandbox")
                    }
                    Failure::WrongKind => operation("delete-first", FIRST, "completed", "snapshot"),
                    Failure::WrongOperation if request.method == "GET" => {
                        operation("delete-other", FIRST, "completed", "sandbox")
                    }
                    Failure::Blocked => operation("delete-first", FIRST, "blocked", "sandbox"),
                    _ => operation("delete-first", FIRST, "pending", "sandbox"),
                });
            }
            if request.method == "GET" && request.path.starts_with("/api/v1/sandboxes/") {
                match failure {
                    Failure::MissingTarget => {
                        return Some(Response(404, json!({"ok":false,"error":"not found"})))
                    }
                    Failure::OperableCompleted => {
                        return Some(Response(
                            200,
                            json!({"ok":true,"sandbox":sandbox(FIRST,"ready")}),
                        ))
                    }
                    _ => {}
                }
            }
            Some(default_response(request))
        });
        let api = server.api();
        ledger.create_target(&api, index, budgets()).await.unwrap();
        assert!(
            ledger.cleanup_all(&api, budgets()).await.is_err(),
            "{failure:?}"
        );
        drop(ledger);
        let reopened = FixtureLedger::load(&path).unwrap();
        assert!(!reopened.fixtures()[index].cleanup.confirmed, "{failure:?}");
        assert_eq!(reopened.fixtures()[index].target_id.as_deref(), Some(FIRST));
        assert!(
            server.requests().len() < 100,
            "cleanup polling must be bounded"
        );
    }
}

#[tokio::test]
async fn cleanup_attempts_every_known_target_after_one_fails() {
    let (_dir, path, mut ledger) = ledger();
    let first = reserve(&mut ledger, "first");
    let second = reserve(&mut ledger, "second");
    let count = Arc::new(Mutex::new(0));
    let server = FakeBoat::start(move |request| {
        if request.method == "POST" && request.path == "/api/v1/sandboxes" {
            let mut count = count.lock().unwrap();
            *count += 1;
            return Some(created(if *count == 1 { FIRST } else { SECOND }));
        }
        if request.method == "DELETE" {
            return Some(if request.path.ends_with(SECOND) {
                operation("delete-second", SECOND, "blocked", "sandbox")
            } else {
                operation("delete-first", FIRST, "completed", "sandbox")
            });
        }
        if request.path.starts_with("/api/v1/deletion-operations/") {
            return Some(operation("delete-second", SECOND, "blocked", "sandbox"));
        }
        Some(default_response(request))
    });
    let api = server.api();
    ledger.create_target(&api, first, budgets()).await.unwrap();
    ledger.create_target(&api, second, budgets()).await.unwrap();
    assert!(ledger.cleanup_all(&api, budgets()).await.is_err());
    drop(ledger);
    let reopened = FixtureLedger::load(&path).unwrap();
    assert!(reopened.fixtures()[first].cleanup.confirmed);
    assert!(!reopened.fixtures()[second].cleanup.confirmed);
    let deleted: Vec<_> = server
        .requests()
        .into_iter()
        .filter(|request| request.method == "DELETE")
        .map(|request| request.path)
        .collect();
    assert_eq!(
        deleted,
        [
            format!("/api/v1/sandboxes/{SECOND}"),
            format!("/api/v1/sandboxes/{FIRST}")
        ]
    );
}

#[tokio::test]
async fn unused_reservation_requires_no_provider_request() {
    let (_dir, path, mut ledger) = ledger();
    let index = reserve(&mut ledger, "reserved-only");
    let server = FakeBoat::start(|_| panic!("reserved intent has no resource to delete"));
    ledger.cleanup_all(&server.api(), budgets()).await.unwrap();
    drop(ledger);
    let reopened = FixtureLedger::load(&path).unwrap();
    assert_eq!(reopened.fixtures()[index].creation, CreationState::Intent);
    assert!(reopened.fixtures()[index].target_id.is_none());
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn reopened_cleanup_polls_saved_operation_and_completes_without_another_delete() {
    let (_dir, path, mut ledger) = ledger();
    let index = reserve(&mut ledger, "resume-delete");
    let completed = Arc::new(AtomicBool::new(false));
    let observation = completed.clone();
    let server = FakeBoat::start(move |request| {
        if request.method == "POST" && request.path == "/api/v1/sandboxes" {
            return Some(created(FIRST));
        }
        if request.method == "DELETE" || request.path == "/api/v1/deletion-operations/delete-first"
        {
            return Some(operation(
                "delete-first",
                FIRST,
                if observation.load(Ordering::Acquire) {
                    "completed"
                } else {
                    "pending"
                },
                "sandbox",
            ));
        }
        Some(default_response(request))
    });
    let api = server.api();
    ledger.create_target(&api, index, budgets()).await.unwrap();
    assert!(ledger.cleanup_all(&api, budgets()).await.is_err());
    drop(ledger);
    let mut reopened = FixtureLedger::load(&path).unwrap();
    assert_eq!(
        reopened.fixtures()[index]
            .cleanup
            .deletion
            .as_ref()
            .unwrap()
            .operation_id,
        "delete-first"
    );
    completed.store(true, Ordering::Release);
    reopened.cleanup_all(&api, budgets()).await.unwrap();
    assert!(persisted_fixtures(&path)[index].cleanup.confirmed);
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|request| request.method == "DELETE")
            .count(),
        1
    );
}

#[tokio::test]
async fn creating_after_unused_reservation_cleanup_cannot_skip_new_target_cleanup() {
    let (_dir, path, mut ledger) = ledger();
    let index = reserve(&mut ledger, "cleaned-intent");
    let server = FakeBoat::start(|request| {
        if request.method == "POST" && request.path == "/api/v1/sandboxes" {
            Some(created(FIRST))
        } else if request.method == "DELETE" {
            Some(operation("delete-first", FIRST, "completed", "sandbox"))
        } else {
            Some(default_response(request))
        }
    });
    let api = server.api();
    ledger.cleanup_all(&api, budgets()).await.unwrap();
    drop(ledger);
    let mut reopened = FixtureLedger::load(&path).unwrap();
    if reopened.create_target(&api, index, budgets()).await.is_ok() {
        reopened.cleanup_all(&api, budgets()).await.unwrap();
        assert_eq!(
            server
                .requests()
                .iter()
                .filter(|request| request.method == "DELETE")
                .count(),
            1,
            "a later created resource must not inherit reservation-only cleanup confirmation"
        );
    } else {
        assert!(
            server.requests().is_empty(),
            "closed intent must reject before creating"
        );
    }
}

#[tokio::test]
async fn exclusive_ledger_owner_rejects_second_opener_and_preserves_new_targets() {
    let (_dir, path, mut ledger) = ledger();
    let first = reserve(&mut ledger, "lock-first");
    let count = Arc::new(Mutex::new(0));
    let server = FakeBoat::start(move |request| {
        if request.method == "POST" && request.path == "/api/v1/sandboxes" {
            let mut count = count.lock().unwrap();
            *count += 1;
            return Some(created(if *count == 1 { FIRST } else { SECOND }));
        }
        if request.method == "DELETE" {
            let target = if request.path.ends_with(FIRST) {
                FIRST
            } else {
                SECOND
            };
            return Some(operation(
                if target == FIRST {
                    "delete-first"
                } else {
                    "delete-second"
                },
                target,
                "completed",
                "sandbox",
            ));
        }
        Some(default_response(request))
    });
    let api = server.api();
    ledger.create_target(&api, first, budgets()).await.unwrap();
    let requests_before = server.requests().len();
    let competing_path = path.clone();
    let (send, receive) = std::sync::mpsc::channel();
    let opener = thread::spawn(move || {
        send.send(FixtureLedger::load(competing_path).is_err())
            .unwrap();
    });
    let rejected_promptly = receive.recv_timeout(Duration::from_millis(500));
    assert_eq!(
        server.requests().len(),
        requests_before,
        "opening an owned ledger must have no provider effects"
    );
    let second = reserve(&mut ledger, "lock-second");
    ledger.create_target(&api, second, budgets()).await.unwrap();
    assert_eq!(
        persisted_fixtures(&path)[second].target_id.as_deref(),
        Some(SECOND)
    );
    drop(ledger);
    opener.join().unwrap();
    assert_eq!(
        rejected_promptly.ok(),
        Some(true),
        "a second owner must fail without waiting for the active owner to release"
    );
    let mut reopened = FixtureLedger::load(&path).unwrap();
    assert_eq!(reopened.fixtures()[first].target_id.as_deref(), Some(FIRST));
    assert_eq!(
        reopened.fixtures()[second].target_id.as_deref(),
        Some(SECOND)
    );
    reopened.cleanup_all(&api, budgets()).await.unwrap();
    assert!(persisted_fixtures(&path)
        .iter()
        .all(|fixture| fixture.cleanup.confirmed));
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|request| request.method == "DELETE")
            .count(),
        2
    );
}

#[tokio::test]
async fn repeated_cleanup_failures_keep_diagnostics_and_ledger_size_bounded() {
    let (_dir, path, mut ledger) = ledger();
    let index = reserve(&mut ledger, "bounded-errors");
    let complete = Arc::new(AtomicBool::new(false));
    let observation = complete.clone();
    let server = FakeBoat::start(move |request| {
        if request.method == "POST" && request.path == "/api/v1/sandboxes" {
            return Some(created(FIRST));
        }
        if !observation.load(Ordering::Acquire) {
            return Some(Response(404, json!({"ok":false,"error":"fixture failure"})));
        }
        if request.method == "DELETE" {
            return Some(operation("delete-first", FIRST, "completed", "sandbox"));
        }
        Some(default_response(request))
    });
    let api = server.api();
    ledger.create_target(&api, index, budgets()).await.unwrap();
    assert!(ledger.cleanup_all(&api, budgets()).await.is_err());
    let initial = fs::read(&path).unwrap();
    let errors = persisted_fixtures(&path)[index].cleanup.errors.clone();
    assert!(!errors.is_empty());
    assert!(errors.len() <= 3, "one failure class per provider phase");
    for _ in 0..9 {
        drop(ledger);
        ledger = FixtureLedger::load(&path).unwrap();
        assert!(ledger.cleanup_all(&api, budgets()).await.is_err());
        assert_eq!(persisted_fixtures(&path)[index].cleanup.errors, errors);
        assert_eq!(
            fs::read(&path).unwrap().len(),
            initial.len(),
            "retries must not accumulate duplicate diagnostic entries"
        );
    }
    complete.store(true, Ordering::Release);
    ledger.cleanup_all(&api, budgets()).await.unwrap();
    drop(ledger);
    let reopened = FixtureLedger::load(&path).unwrap();
    assert!(reopened.fixtures()[index].cleanup.confirmed);
    assert_eq!(reopened.fixtures()[index].target_id.as_deref(), Some(FIRST));
    assert_eq!(reopened.fixtures()[index].cleanup.errors, errors);
}

#[tokio::test]
async fn abandoned_connections_do_not_kill_server_or_record_provider_effects() {
    let (_dir, path, mut ledger) = ledger();
    let index = reserve(&mut ledger, "after-abandoned-connections");
    let server = FakeBoat::start(|request| {
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/api/v1/sandboxes");
        Some(created(FIRST))
    });
    // A canceled request can close before any bytes, during its headers, or
    // before its declared body is complete. None is a delivered API request.
    for partial in [
        &b""[..],
        &b"POST /api/v1/sandboxes HTTP/1.1\r\nContent-Len"[..],
        &b"POST /api/v1/sandboxes HTTP/1.1\r\nContent-Length: 20\r\n\r\n{"[..],
    ] {
        let mut stream = TcpStream::connect(&server.address).unwrap();
        stream.write_all(partial).unwrap();
        stream.shutdown(std::net::Shutdown::Both).unwrap();
    }
    let target = ledger
        .create_target(&server.api(), index, budgets())
        .await
        .unwrap();
    assert_eq!(target, FIRST);
    let persisted = persisted_fixtures(&path);
    assert_eq!(persisted.len(), 1);
    assert_eq!(persisted[index].creation, CreationState::Created);
    assert_eq!(persisted[index].target_id.as_deref(), Some(FIRST));
    let delivered = server.requests();
    assert_eq!(
        delivered.len(),
        1,
        "incomplete connections must not reach the handler"
    );
    assert_eq!(delivered[0].method, "POST");
    assert_eq!(delivered[0].path, "/api/v1/sandboxes");
    assert_eq!(delivered[0].body, json!({"ttlSeconds":300,"noEnv":true}));
}
