#![cfg(feature = "agent37")]
//! Agent37 attachment against a fake Sandbox API whose `exec` runs each
//! command on this host, so the real supervisor answers. It does not reach
//! Agent37, and its tunnel leads nowhere: what needs a connection is shown
//! to fail closed, never to fall back to a command line.
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use workspace_sdk::{
    providers::{
        agent37::{Agent37AttachOptions, Agent37Config, Agent37Identity, Agent37Provider, ApiKey},
        SupervisorDeployment,
    },
    *,
};

const INSTANCE: &str = "ab12cd34ef";

#[derive(Default)]
struct Fake {
    status: String,
    commands: Vec<String>,
}
type Shared = Arc<Mutex<Fake>>;

async fn instance(State(fake): State<Shared>, Path(id): Path<String>) -> Response {
    if id != INSTANCE {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"code": "not_found"}})),
        )
            .into_response();
    }
    let status = fake.lock().unwrap().status.clone();
    Json(
        json!({"id": id, "status": status, "template": "nessa-runtime@1",
        "image_digest": "sha256:x"}),
    )
    .into_response()
}
async fn exec(
    State(fake): State<Shared>,
    Path(_id): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let command = body["command"].as_str().unwrap().to_owned();
    fake.lock().unwrap().commands.push(command.clone());
    let ran = tokio::process::Command::new("sh")
        .args(["-c", &command])
        .current_dir("/")
        .output()
        .await
        .unwrap();
    Json(json!({
        "exit_code": ran.status.code().unwrap_or(-1),
        "stdout": String::from_utf8_lossy(&ran.stdout),
        "stderr": String::from_utf8_lossy(&ran.stderr),
        "truncated": false,
    }))
}

struct Server {
    fake: Shared,
    provider: Agent37Provider,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn server(status: &str) -> Server {
    let fake = Shared::default();
    fake.lock().unwrap().status = status.into();
    let router = Router::new()
        .route("/v1/instances/{id}", get(instance))
        .route("/v1/instances/{id}/exec", post(exec))
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    // A tunnel to a port nothing listens on.
    let config = Agent37Config::new(ApiKey::new("sk_live_fixture").unwrap())
        .with_api_base(&format!("http://{address}/v1"))
        .unwrap()
        .with_tunnel("ws://127.0.0.1:9/{id}")
        .unwrap();
    let identity = Agent37Identity::generate("contract").await.unwrap();
    let provider = Agent37Provider::new(
        config,
        identity,
        SupervisorDeployment::Preinstalled {
            executable: env!("CARGO_BIN_EXE_workspace-supervisor").into(),
        },
    )
    .unwrap();
    Server {
        fake,
        provider,
        task,
    }
}

/// A place with no symbolic link on its path, as the supervisor requires.
fn place() -> (tempfile::TempDir, std::path::PathBuf) {
    let place = tempfile::tempdir().unwrap();
    let real = place.path().canonicalize().unwrap();
    (place, real)
}

fn attach(root: &std::path::Path) -> AttachRequest {
    AttachRequest::new(
        WorkspaceId::generate(),
        &Agent37AttachOptions {
            instance_id: INSTANCE.into(),
            root: root.to_str().unwrap().into(),
        },
    )
    .unwrap()
}

#[tokio::test]
async fn an_attached_instance_runs_the_supervisor_through_exec_and_reconnects() {
    let server = server("running").await;
    let (_place, real) = place();
    let root = real.join("project");
    let backend = server.provider.attach(attach(&root)).await.unwrap();
    assert!(root.is_dir(), "Initialize made the project");
    let descriptor = backend.descriptor();
    assert_eq!(descriptor.ownership, Ownership::Attached);
    assert!(backend.connections().is_some());
    assert!(backend.lifecycle().is_none() && backend.deletion().is_none());
    std::fs::write(root.join("note.txt"), b"hello").unwrap();
    let read = backend
        .read_file(ReadFile {
            path: WorkspacePath::new("note.txt").unwrap(),
            offset: 0,
            limit: 64,
        })
        .await
        .unwrap();
    assert_eq!(read.bytes, b"hello");
    let again = server
        .provider
        .connect(&descriptor.reference)
        .await
        .unwrap();
    assert_eq!(again.descriptor().root, descriptor.root);
    // Nothing secret went through a command line: these were all inline.
    assert!(server
        .fake
        .lock()
        .unwrap()
        .commands
        .iter()
        .all(|c| c.starts_with("printf")));
}

#[tokio::test]
async fn what_needs_a_private_channel_fails_closed_without_one() {
    let server = server("running").await;
    let (_place, real) = place();
    let backend = server
        .provider
        .attach(attach(&real.join("project")))
        .await
        .unwrap();
    let before = server.fake.lock().unwrap().commands.len();
    let written = backend
        .write_file(WriteFile {
            path: WorkspacePath::new("secret.txt").unwrap(),
            offset: 0,
            bytes: b"secret".to_vec(),
            truncate: true,
            executable: false,
        })
        .await;
    assert!(written.is_err());
    let commands = server.fake.lock().unwrap().commands.clone();
    assert_eq!(commands.len(), before, "nothing fell back to exec");
    assert!(!real.join("project/secret.txt").exists());
}

#[tokio::test]
async fn a_stopped_instance_or_a_bad_option_is_refused_before_anything_runs() {
    let server = server("stopped").await;
    let (_place, real) = place();
    assert!(server
        .provider
        .attach(attach(&real.join("project")))
        .await
        .is_err());
    assert!(server.fake.lock().unwrap().commands.is_empty());
    for (instance, root) in [("AB12", "/x/y"), (INSTANCE, "relative"), (INSTANCE, "/")] {
        let request = AttachRequest::new(
            WorkspaceId::generate(),
            &Agent37AttachOptions {
                instance_id: instance.into(),
                root: root.into(),
            },
        )
        .unwrap();
        assert!(
            server.provider.check_attach(&request).is_err(),
            "{instance} {root}"
        );
    }
    assert!(server
        .provider
        .check_create(&CreateRequest::new(WorkspaceId::generate()))
        .is_err());
}
