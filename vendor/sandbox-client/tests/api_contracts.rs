//! Request, response, and convenience-helper contracts.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use boat_client::{
    exec_command, read_text, wait_until_ready, wait_until_ready_with, write_text, BoatApi,
    BoatClientConfig, CommandRequest, CreateOptions, CreateSandboxRequest, DesktopQuery, Error,
    FileWriteRequest, HostPortRequest, ResumeRequest, SandboxesQuery, StopRequest,
    UpdateSandboxRequest, WaitOptions,
};
use serde_json::json;
use wiremock::matchers::{body_partial_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn fixture(name: &str) -> serde_json::Value {
    serde_json::from_str::<serde_json::Value>(include_str!("fixtures/runtime.json")).unwrap()[name]
        .clone()
}

fn box_json(id: &str, state: &str) -> serde_json::Value {
    json!({
        "id": id,
        "name": "demo",
        "state": state,
        "desktopAvailable": false,
        "snapshotAvailable": false,
        "type": "default",
        "vcpu": 4,
        "memoryGB": 8
    })
}

async fn api(server: &MockServer) -> BoatApi {
    BoatApi::new(
        BoatClientConfig::new("box_test_key")
            .unwrap()
            .with_base_path(server.uri())
            .unwrap(),
    )
    .unwrap()
}

// Account operations.

#[tokio::test]
async fn me() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/me"))
        .and(header("Authorization", "Bearer box_test_key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "type": "account.me",
            "user": { "login": "dev@example.com", "email": "dev@example.com" }
        })))
        .mount(&server)
        .await;

    let me = api(&server).await.me().await.unwrap();
    assert_eq!(me.user.email.as_deref(), Some("dev@example.com"));
}

#[tokio::test]
async fn limits() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/limits"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "canStart": true,
            "activeSandboxes": 0, "maxActiveSandboxes": 100, "billingStatus":"active", "type":"account.limits"
        })))
        .mount(&server)
        .await;

    let limits = api(&server)
        .await
        .limits(&Default::default())
        .await
        .unwrap();
    assert!(limits.ok);
    assert!(limits.can_start);
}

// Sandbox lifecycle.

#[tokio::test]
async fn boxes_list_and_query() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/sandboxes"))
        .and(query_param("state", "idle"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "type": "box.list",
            "sandboxes": [box_json("bx_23456789", "idle")],
            "pageInfo": { "nextCursor": null, "hasMore": false, "limit": 50 }
        })))
        .mount(&server)
        .await;

    let q = SandboxesQuery {
        state: Some("idle".into()),
        limit: Some(50),
        ..Default::default()
    };
    let list = api(&server).await.sandboxes(Some(&q)).await.unwrap();
    assert_eq!(list.sandboxes.len(), 1);
    assert_eq!(list.sandboxes[0].id, "bx_23456789");
}

#[tokio::test]
async fn create_with_idempotency_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/sandboxes"))
        .and(header("Idempotency-Key", "job-42"))
        .and(body_partial_json(json!({ "ttlSeconds": 1800 })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "type": "box.created",
            "status": "provisioning",
            "ttlSeconds": 1800,
            "sandbox": box_json("bx_23456789", "provisioning")
        })))
        .mount(&server)
        .await;

    let created = api(&server)
        .await
        .create(
            Some(CreateSandboxRequest::ttl(1800)),
            &CreateOptions {
                idempotency_key: Some("job-42".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(created.sandbox.id, "bx_23456789");
}

#[tokio::test]
async fn extended_prompt_events_desktop_and_delete_routes() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/sandboxes/bx_23456789/prompt"))
        .and(body_partial_json(json!({"prompt": "do it", "provider":"codex"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "id":"bx_23456789", "promptId": "pr_1", "status": "queued", "provider":"codex",
            "promptRun":{"id":"pr_1","promptId":"pr_1","sandboxId":"bx_23456789","status":"queued","done":false}
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/sandboxes/bx_23456789/events"))
        .and(query_param("sort", "asc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "id":"bx_23456789", "events": [{"id":"ev_1","type":"prompt","timestamp":1,"data":{}}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/sandboxes/bx_23456789/desktop"))
        .and(query_param("vnc", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "desktopUrl": "https://secret.example", "provisioning": false
        })))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/sandboxes/bx_23456789"))
        .and(header("X-Ascii-Confirm-Delete", "bx_23456789"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("deletion")))
        .mount(&server)
        .await;

    let client = api(&server).await;
    let prompt = client
        .prompt(
            "bx_23456789",
            boat_client::PromptRequest::new(boat_client::PromptProvider::Codex, "do it"),
        )
        .await
        .unwrap();
    assert_eq!(prompt.prompt_id, "pr_1");
    let events = client
        .events(
            "bx_23456789",
            Some(&boat_client::EventsQuery {
                sort: Some("asc".into()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    assert_eq!(events.events[0].id.as_deref(), Some("ev_1"));
    let desktop = client
        .desktop(
            "bx_23456789",
            Some(&DesktopQuery {
                vnc: Some(1),
                ..Default::default()
            }),
            Some(Default::default()),
        )
        .await
        .unwrap();
    assert!(desktop.desktop_url.is_some());
    client.delete_sandbox("bx_23456789").await.unwrap();
}

#[tokio::test]
async fn extended_snapshot_and_environment_routes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/snapshots/snap%2Funsafe/tree"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("snapshotTree")))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/environments"))
        .and(body_partial_json(json!({"name":"ci"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("environments")))
        .mount(&server)
        .await;

    let client = api(&server).await;
    client.snapshot_tree("snap/unsafe").await.unwrap();
    let envs = client
        .create_environment(boat_client::CreateSandboxEnvironmentRequest { name: "ci".into() })
        .await
        .unwrap();
    assert_eq!(
        envs.environments[0].id,
        "11111111-2222-3333-4444-555555555555"
    );
}

#[tokio::test]
async fn get_update_stop_resume() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/sandboxes/bx_23456789"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "type": "box.info", "sandbox": box_json("bx_23456789", "idle")
        })))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/sandboxes/bx_23456789"))
        .and(body_partial_json(json!({ "name": "sdk-demo" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "type": "box.info",
            "sandbox": { "id": "bx_23456789", "name": "sdk-demo", "state": "idle",
                      "desktopAvailable": false, "snapshotAvailable": false }
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/sandboxes/bx_23456789/stop"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "type": "box.stopping", "id": "bx_23456789", "status": "archiving"
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/sandboxes/bx_23456789/resume"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "type": "box.resuming", "id": "bx_23456789", "status": "provisioning"
        })))
        .mount(&server)
        .await;

    let client = api(&server).await;
    assert_eq!(
        client
            .get("bx_23456789")
            .await
            .unwrap()
            .sandbox
            .state
            .as_str(),
        "idle"
    );
    assert_eq!(
        client
            .update(
                "bx_23456789",
                UpdateSandboxRequest {
                    name: Some("sdk-demo".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .sandbox
            .name
            .as_deref(),
        Some("sdk-demo")
    );
    assert_eq!(
        client
            .stop("bx_23456789", Some(StopRequest::default()))
            .await
            .unwrap()
            .status,
        "archiving"
    );
    assert_eq!(
        client
            .resume("bx_23456789", Some(ResumeRequest::default()))
            .await
            .unwrap()
            .status,
        "provisioning"
    );
}

// Convenience helpers.

#[tokio::test]
async fn wait_until_ready_success_states() {
    for state in ["ready", "idle", "running"] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sandboxes/bx_23456789"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok": true, "type": "box.info", "sandbox": box_json("bx_23456789", state)
            })))
            .mount(&server)
            .await;
        let sandbox = wait_until_ready(&api(&server).await, "bx_23456789")
            .await
            .unwrap();
        assert_eq!(sandbox.state.as_str(), state);
    }
}

#[tokio::test]
async fn wait_until_ready_terminal_states() {
    for state in ["archived", "archiving", "error"] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sandboxes/bx_23456789"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok": true, "type": "box.info", "sandbox": box_json("bx_23456789", state)
            })))
            .mount(&server)
            .await;
        let err = wait_until_ready(&api(&server).await, "bx_23456789")
            .await
            .unwrap_err();
        match err {
            Error::SandboxTerminal { state: s, .. } => assert_eq!(s, state),
            other => panic!("expected SandboxTerminal, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn wait_until_ready_polls_then_succeeds() {
    let server = MockServer::start().await;
    let hits = Arc::new(AtomicUsize::new(0));
    let hits2 = hits.clone();
    Mock::given(method("GET"))
        .and(path("/sandboxes/bx_23456789"))
        .respond_with(move |_req: &Request| {
            let n = hits2.fetch_add(1, Ordering::SeqCst);
            let state = if n == 0 { "provisioning" } else { "ready" };
            ResponseTemplate::new(200).set_body_json(json!({
                "ok": true, "type": "box.info", "sandbox": box_json("bx_23456789", state)
            }))
        })
        .mount(&server)
        .await;

    let sandbox = wait_until_ready_with(
        &api(&server).await,
        "bx_23456789",
        WaitOptions {
            timeout: Duration::from_secs(5),
            poll_interval: Duration::from_millis(20),
        },
    )
    .await
    .unwrap();
    assert_eq!(sandbox.state.as_str(), "ready");
    assert!(hits.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn exec_command_default_timeout_30() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/sandboxes/bx_23456789/commands"))
        .and(body_partial_json(json!({
            "command": "uname -a",
            "timeoutSeconds": 30
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "type": "command.finished",
            "success": true,
            "exitCode": 0,
            "stdout": "Linux\n",
            "stderr": "",
            "timedOut": false
        })))
        .mount(&server)
        .await;

    let out = exec_command(&api(&server).await, "bx_23456789", "uname -a", None, None)
        .await
        .unwrap();
    assert_eq!(out.stdout, "Linux\n");
    assert_eq!(out.exit_code, Some(0));
}

#[tokio::test]
async fn exec_command_with_cwd_and_timeout() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/sandboxes/bx_23456789/commands"))
        .and(body_partial_json(json!({
            "command": "pwd",
            "cwd": "project",
            "timeoutSeconds": 60
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "type": "command.finished", "success": true,
            "exitCode": 0, "stdout": "/home/user/project\n", "stderr": "", "timedOut": false
        })))
        .mount(&server)
        .await;

    let out = exec_command(
        &api(&server).await,
        "bx_23456789",
        "pwd",
        Some("project"),
        Some(60),
    )
    .await
    .unwrap();
    assert!(out.stdout.contains("project"));
}

#[tokio::test]
async fn read_text_write_text_utf8() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/sandboxes/bx_23456789/files"))
        .and(query_param("path", "notes/result.txt"))
        .and(query_param("encoding", "utf8"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "path": "notes/result.txt",
            "content": "done\n",
            "encoding": "utf8"
        })))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/sandboxes/bx_23456789/files"))
        .and(body_partial_json(json!({
            "path": "notes/result.txt",
            "content": "done\n",
            "encoding": "utf8"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "success": true
        })))
        .mount(&server)
        .await;

    let client = api(&server).await;
    write_text(&client, "bx_23456789", "notes/result.txt", "done\n")
        .await
        .unwrap();
    let text = read_text(&client, "bx_23456789", "notes/result.txt")
        .await
        .unwrap();
    assert_eq!(text, "done\n");
}

#[tokio::test]
async fn host_port_and_ssh_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/sandboxes/bx_23456789/host"))
        .and(body_partial_json(json!({ "port": 3000 })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "type": "port.hosted", "success": true, "port": 3000,
            "url": "https://example.on.ascii.dev?_token=redacted"
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/sandboxes/bx_23456789/sshkey"))
        .and(body_partial_json(json!({ "key": "ssh-ed25519 AAAA" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "type": "ssh_key.configured", "success": true,
            "machineIp": "203.0.113.10", "sshUser": "user"
        })))
        .mount(&server)
        .await;

    let client = api(&server).await;
    let host = client
        .host_port(
            "bx_23456789",
            HostPortRequest {
                port: 3000,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(host.port, Some(3000));
    let ssh = client
        .ssh_key("bx_23456789", "ssh-ed25519 AAAA")
        .await
        .unwrap();
    assert_eq!(ssh.ssh_user.as_deref(), Some("user"));
}

#[tokio::test]
async fn invalid_box_id_is_rejected() {
    // Invalid identifiers are rejected before sending a request.
    let client = BoatApi::new(BoatClientConfig::new("box_test_key").unwrap()).unwrap();
    let err = client
        .command("bad", CommandRequest::new("true"))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::InvalidSandboxId(_)));
}

#[tokio::test]
async fn api_error_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/sandboxes/bx_23456789"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "ok": false,
            "type": "box.error",
            "status": 404,
            "code": "not_found",
            "message": "Sandbox not found",
            "requestId": "req_abc",
            "error": { "code": "not_found", "message": "Sandbox not found", "status": 404 }
        })))
        .mount(&server)
        .await;

    let err = api(&server).await.get("bx_23456789").await.unwrap_err();
    match &err {
        Error::Api {
            status,
            code,
            request_id,
            ..
        } => {
            assert_eq!(*status, 404);
            assert_eq!(code, "not_found");
            assert_eq!(request_id, "req_abc");
            assert_eq!(err.api_message(), Some("Sandbox not found"));
            // Display stays log-safe (no server message body).
            assert!(!err.to_string().contains("Sandbox not found"));
        }
        other => panic!("expected Api error, got {other:?}"),
    }
}

#[tokio::test]
async fn rejects_detached_command() {
    let client = BoatApi::new(BoatClientConfig::new("box_test_key").unwrap()).unwrap();
    let err = client
        .command(
            "bx_23456789",
            CommandRequest {
                command: "sleep 1".into(),
                cwd: None,
                timeout_seconds: Some(30),
                detached: Some(true),
                stream: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Config(_)));
}

#[tokio::test]
async fn create_request_ttl_preserves_null_and_omission() {
    assert_eq!(
        serde_json::to_value(CreateSandboxRequest::ttl(1800)).unwrap(),
        json!({ "ttlSeconds": 1800 })
    );
    assert_eq!(
        serde_json::to_value(CreateSandboxRequest::no_auto_stop()).unwrap(),
        json!({ "ttlSeconds": null })
    );
}

#[tokio::test]
async fn write_file_uses_put_and_serializes_body() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/sandboxes/bx_23456789/files"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "ok": true, "success": true })),
        )
        .mount(&server)
        .await;
    api(&server)
        .await
        .write_file(
            "bx_23456789",
            FileWriteRequest {
                path: "a.txt".into(),
                content: "x".into(),
                encoding: Some("utf8".into()),
            },
        )
        .await
        .unwrap();
}
