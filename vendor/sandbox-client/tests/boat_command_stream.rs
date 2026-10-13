//! Command methods return one JSON result. Boat's synchronous `stream: true`
//! selects NDJSON, while detached commands ignore stream and return started JSON.
use boat_client::{BoatApi, BoatClientConfig, CommandRequest, CommandResult, Error};
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SANDBOX_ID: &str = "bx_23456789";

fn client(server: &MockServer) -> BoatApi {
    BoatApi::new(
        BoatClientConfig::new("fixture-only")
            .unwrap()
            .with_base_path(server.uri())
            .unwrap(),
    )
    .unwrap()
}

async fn assert_sync_stream_rejected_before_http(raw: bool) {
    for detached in [None, Some(false)] {
        let server = MockServer::start().await;
        // If the client submits this request, Boat may already execute it before
        // decoding the multiple JSON records fails. Rejection must precede POST.
        Mock::given(method("POST"))
            .and(path(format!("/sandboxes/{SANDBOX_ID}/commands")))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                "{\"type\":\"started\"}\n{\"type\":\"exit\",\"exitCode\":0,\"success\":true}\n",
                "application/x-ndjson",
            ))
            .mount(&server)
            .await;
        let mut request = CommandRequest::new("printf 'fixture'");
        request.stream = Some(true);
        request.detached = detached;
        let api = client(&server);
        let result = if raw {
            api.command_raw(SANDBOX_ID, request).await.map(|_| ())
        } else {
            api.command(SANDBOX_ID, request).await.map(|_| ())
        };
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            0,
            "unsupported stream must be rejected before command submission (raw={raw}, detached={detached:?})"
        );
        assert!(
            matches!(result, Err(Error::Config(_))),
            "unsupported local response mode must return Config, got {result:?}"
        );
    }
}

#[tokio::test]
async fn command_rejects_sync_stream_before_submitting_the_command() {
    assert_sync_stream_rejected_before_http(false).await;
}

#[tokio::test]
async fn command_raw_rejects_sync_stream_before_submitting_the_command() {
    assert_sync_stream_rejected_before_http(true).await;
}

#[tokio::test]
async fn detached_stream_flag_preserves_single_post_and_started_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/sandboxes/{SANDBOX_ID}/commands")))
        .and(body_json(json!({
            "command": "printf 'fixture'", "detached": true, "stream": true
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "type": "command.started",
            "success": true,
            "processId": 123,
            "pid": 456,
            "command": "printf 'fixture'",
            "startedAt": "2026-10-12T00:00:00Z"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let mut request = CommandRequest::new("printf 'fixture'");
    request.detached = Some(true);
    request.stream = Some(true);
    let result = client(&server)
        .command_raw(SANDBOX_ID, request)
        .await
        .unwrap();
    let CommandResult::Started(started) = result else {
        panic!("detached response must preserve its started variant");
    };
    assert!(started.ok && started.success);
    assert_eq!(started.type_.as_deref(), Some("command.started"));
    assert_eq!(started.process_id, 123);
    assert_eq!(started.pid, 456);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn explicit_false_stream_keeps_the_synchronous_json_contract() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/sandboxes/{SANDBOX_ID}/commands")))
        .and(body_json(
            json!({"command": "printf 'fixture'", "stream": false}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "type": "command.finished",
            "success": true,
            "exitCode": 0,
            "stdout": "fixture",
            "stderr": "",
            "timedOut": false
        })))
        .expect(1)
        .mount(&server)
        .await;
    let mut request = CommandRequest::new("printf 'fixture'");
    request.stream = Some(false);
    let completed = client(&server).command(SANDBOX_ID, request).await.unwrap();
    assert_eq!(completed.stdout, "fixture");
    assert_eq!(completed.exit_code, Some(0));
    assert!(completed.success);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
