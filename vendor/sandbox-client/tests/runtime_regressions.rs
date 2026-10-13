//! Regression coverage for runtime behavior, response models, and transport limits.
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use boat_client::*;
use serde_json::{json, Value};
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const SANDBOX_ID: &str = "bx_23456789";
const ENV_ID: &str = "11111111-2222-3333-4444-555555555555";
fn fixture(name: &str) -> Value {
    serde_json::from_str::<Value>(include_str!("fixtures/runtime.json")).unwrap()[name].clone()
}
fn client(server: &MockServer) -> BoatApi {
    BoatApi::new(
        BoatClientConfig::new("test-key")
            .unwrap()
            .with_base_path(server.uri())
            .unwrap(),
    )
    .unwrap()
}
fn box_info(state: &str) -> Value {
    json!({"ok":true,"sandbox":{"id":SANDBOX_ID,"name":"test","state":state}})
}

#[tokio::test]
async fn identifiers_work_across_environment_snapshot_and_prompt_routes() {
    let server = MockServer::start().await;
    for verb in ["PUT", "DELETE"] {
        Mock::given(method(verb))
            .and(path(format!("/environments/{ENV_ID}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(fixture("environment")))
            .expect(1)
            .mount(&server)
            .await;
    }
    for (route, body) in [("tree", "snapshotTree"), ("download", "snapshotDownload")] {
        Mock::given(method("GET"))
            .and(path(format!("/snapshots/snap_test-1/{route}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(fixture(body)))
            .expect(1)
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path(format!("/sandboxes/{SANDBOX_ID}/prompts/pr_test-1")))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("promptRun")))
        .expect(1)
        .mount(&server)
        .await;
    let api = client(&server);
    assert!(
        api.update_environment(ENV_ID, UpdateSandboxEnvironmentRequest::default())
            .await
            .unwrap()
            .success
    );
    assert!(api.delete_environment(ENV_ID).await.unwrap().success);
    assert_eq!(
        api.snapshot_tree("snap_test-1").await.unwrap().entries[0].size,
        Some(10)
    );
    let download = api.snapshot_download("snap_test-1").await.unwrap();
    assert_eq!(download.chunks[0].chunk_index, 0);
    assert!(download.inventory.is_some());
    assert_eq!(
        api.prompt_run_status(SANDBOX_ID, "pr_test-1")
            .await
            .unwrap()
            .prompt_run
            .status,
        "queued"
    );
}

#[tokio::test]
async fn dot_segments_are_rejected_without_requests_and_delimiters_stay_in_segment() {
    let server = MockServer::start().await;
    let api = client(&server);
    for id in ["", " ", ".", ".."] {
        assert!(matches!(
            api.delete_environment(id).await,
            Err(Error::Config(_))
        ));
        assert!(matches!(
            api.snapshot_download(id).await,
            Err(Error::Config(_))
        ));
        assert!(matches!(
            api.get_deletion_operation(id).await,
            Err(Error::Config(_))
        ));
    }
    assert!(server.received_requests().await.unwrap().is_empty());
    Mock::given(method("GET"))
        .and(path("/snapshots/a%2Fb%3Fc%23d%252e/tree"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("snapshotTree")))
        .expect(1)
        .mount(&server)
        .await;
    api.snapshot_tree("a/b?c#d%2e").await.unwrap();
}

#[tokio::test]
async fn snapshot_file_path_is_a_query_and_binary_is_preserved() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/snapshots/snap_test-1/files"))
        .and(query_param("path", "/home/user/a b?#.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0, 255, 1]))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        client(&server)
            .snapshot_file("snap_test-1", Some("/home/user/a b?#.bin"))
            .await
            .unwrap(),
        vec![0, 255, 1]
    );
}

#[test]
fn nested_debug_hides_secrets_but_explicit_payload_access_remains() {
    let env: SandboxEnvironmentListResponse =
        serde_json::from_value(fixture("environments")).unwrap();
    assert_eq!(env.environments[0].env_contents, "TOKEN=secret-env");
    assert_eq!(env.environments[0].secret_files[0].contents, "secret-file");
    assert_eq!(
        env.environments[0].selected_repositories.as_ref().unwrap()[0].private,
        Some(true)
    );
    let event: EventsResponse = serde_json::from_value(fixture("events")).unwrap();
    assert!(event.events[0].id.is_none());
    assert!(event.events[0].timestamp.is_none());
    assert_eq!(event.events[0].extra["futureField"], "secret-extension");
    assert_eq!(
        event.events[0].data.as_ref().unwrap()["text"],
        "secret-output"
    );
    let info: SandboxInfoResponse = serde_json::from_value(json!({"ok":true,"sandbox":{
        "id":SANDBOX_ID,"name":"test","state":"idle","desktopUrl":"https://example.test/?token=secret-box",
        "error":{"details":"secret-error"}
    }})).unwrap();
    assert!(!format!("{info:?}").contains("secret-"));
    let download: SnapshotDownloadResponse =
        serde_json::from_value(fixture("snapshotDownload")).unwrap();
    let started: CommandStartedResponse =
        serde_json::from_value(fixture("commandStarted")).unwrap();
    let status: CommandStatusResponse = serde_json::from_value(fixture("commandStatus")).unwrap();
    for debug in [
        format!("{env:?}"),
        format!("{:?}", env.environments[0]),
        format!("{:?}", env.environments[0].secret_files[0]),
        format!("{:?}", env.environments[0].selected_repositories),
        format!("{event:?}"),
        format!("{:?}", event.events[0]),
        format!("{download:?}"),
        format!("{:?}", download.chunks[0]),
        format!("{:?}", download.inventory),
        format!("{started:?}"),
        format!("{status:?}"),
    ] {
        assert!(!debug.contains("secret-"), "payload leaked: {debug}");
    }
    let mut response = fixture("prompt");
    response["unrecognized"] = json!("secret-extra");
    let prompt: PromptResponse = serde_json::from_value(response).unwrap();
    assert!(!format!("{prompt:?}").contains("secret-extra"));
    let bare: SandboxEvent =
        serde_json::from_value(json!({"type":"new_event","unknown":{"token":"secret"}})).unwrap();
    assert!(bare.data.is_none());
    assert!(bare.extra.contains_key("unknown"));
}

#[tokio::test]
async fn prompt_constructor_emits_required_provider() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/sandboxes/{SANDBOX_ID}/prompt")))
        .and(body_json(json!({"provider":"codex","prompt":"do it"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("prompt")))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        client(&server)
            .prompt(
                SANDBOX_ID,
                PromptRequest::new(PromptProvider::Codex, "do it")
            )
            .await
            .unwrap()
            .provider,
        "codex"
    );
    for (p, wire) in [
        (PromptProvider::ClaudeCode, "claude-code"),
        (PromptProvider::Claude, "claude"),
    ] {
        assert_eq!(
            serde_json::to_value(PromptRequest::new(p, "x")).unwrap()["provider"],
            wire
        );
    }
}

#[tokio::test]
async fn snapshot_pagination_preserves_cursor_limit_and_sort_for_both_routes() {
    let server = MockServer::start().await;
    for route in [
        "/snapshots".to_string(),
        format!("/sandboxes/{SANDBOX_ID}/snapshots"),
    ] {
        Mock::given(method("GET"))
            .and(path(route))
            .and(query_param("limit", "1"))
            .and(query_param("sort", "asc"))
            .respond_with(|req: &Request| {
                let mut body = fixture("snapshots");
                if req
                    .url
                    .query_pairs()
                    .any(|(k, v)| k == "cursor" && v == "cursor/+=")
                {
                    body["snapshots"][0]["id"] = json!("snap_second");
                    body["pageInfo"] = json!({"hasMore":false,"limit":1,"nextCursor":null});
                }
                ResponseTemplate::new(200).set_body_json(body)
            })
            .expect(2)
            .mount(&server)
            .await;
    }
    let api = client(&server);
    for per_box in [false, true] {
        let mut q = SnapshotsQuery {
            limit: Some(1),
            sort: Some("asc".into()),
            ..Default::default()
        };
        let first = if per_box {
            api.list_sandbox_snapshots(SANDBOX_ID, Some(&q)).await
        } else {
            api.list_snapshots(Some(&q)).await
        }
        .unwrap();
        let page = first.page_info.unwrap();
        assert!(page.has_more);
        q.cursor = page.next_cursor;
        let second = if per_box {
            api.list_sandbox_snapshots(SANDBOX_ID, Some(&q)).await
        } else {
            api.list_snapshots(Some(&q)).await
        }
        .unwrap();
        assert_eq!(second.snapshots[0].id, "snap_second");
        assert!(!second.page_info.unwrap().has_more);
    }
    assert!(server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .all(|r| !r.url.query_pairs().any(|(k, _)| k == "sandboxId")));
}

#[tokio::test]
async fn command_union_and_status_preserve_truncation_and_recovery_fields() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/sandboxes/{SANDBOX_ID}/commands")))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("commandStarted")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/sandboxes/{SANDBOX_ID}/commands/123")))
        .and(query_param("tailBytes", "4096"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("commandStatus")))
        .expect(1)
        .mount(&server)
        .await;
    let api = client(&server);
    let mut q = CommandRequest::new("hello");
    q.detached = Some(true);
    let CommandResult::Started(started) = api.command_raw(SANDBOX_ID, q).await.unwrap() else {
        panic!("lost detached union variant")
    };
    assert_eq!(started.process_id, 123);
    assert_eq!(started.log_path.as_deref(), Some("/tmp/stdout"));
    let status = api
        .command_status(
            SANDBOX_ID,
            "123",
            Some(&CommandStatusQuery {
                tail_bytes: Some(4096),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    assert_eq!(status.status, "lost");
    assert_eq!(status.known, Some(false));
    assert!(!status.running);
    assert_eq!(status.stdout_truncated, Some(true));
    assert_eq!(status.stderr_truncated, Some(true));
    assert_eq!(status.log_path.as_deref(), Some("/tmp/stdout"));
    assert_eq!(status.err_log_path.as_deref(), Some("/tmp/stderr"));
}

#[tokio::test]
async fn stop_or_delete_retains_original_response_and_confirmation_is_not_retried() {
    let server = MockServer::start().await;
    let api = client(&server);
    Mock::given(method("POST"))
        .and(path(format!("/sandboxes/{SANDBOX_ID}/stop")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"id":SANDBOX_ID,"status":"archiving"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let StopOrDeleteResponse::Stopped(stopped) =
        stop_and_remove_with(&api, SANDBOX_ID, false).await.unwrap()
    else {
        panic!("wrong branch")
    };
    assert_eq!(stopped.id, SANDBOX_ID);
    assert_eq!(stopped.status, "archiving");
    Mock::given(method("DELETE"))
        .and(path(format!("/sandboxes/{SANDBOX_ID}")))
        .and(header("X-Ascii-Confirm-Delete", SANDBOX_ID))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("deletion")))
        .expect(1)
        .mount(&server)
        .await;
    let StopOrDeleteResponse::Deleting(deleting) =
        stop_and_remove_with(&api, SANDBOX_ID, true).await.unwrap()
    else {
        panic!("wrong branch")
    };
    assert_eq!(deleting.operation.status, "pending");
    Mock::given(method("DELETE"))
        .and(path("/snapshots/snap_test-1"))
        .and(header("X-Ascii-Confirm-Delete", "snap_test-1"))
        .respond_with(
            ResponseTemplate::new(503).set_body_json(json!({"ok":false,"code":"unavailable"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    assert!(api.delete_snapshot("snap_test-1").await.is_err());
}

#[tokio::test]
async fn deletion_wait_observes_blocked_processing_then_completed() {
    let server = MockServer::start().await;
    let hits = Arc::new(AtomicUsize::new(0));
    let seen = hits.clone();
    Mock::given(method("GET"))
        .and(path("/deletion-operations/del_test-1"))
        .respond_with(move |_: &Request| {
            let n = seen.fetch_add(1, Ordering::SeqCst);
            let mut body = fixture("deletion");
            body["operation"]["status"] = json!(match n {
                0 => "blocked",
                1 => "processing",
                _ => "completed",
            });
            ResponseTemplate::new(200).set_body_json(body)
        })
        .expect(3)
        .mount(&server)
        .await;
    let operation = wait_for_deletion(
        &client(&server),
        "del_test-1",
        Some(WaitOptions::from_millis(2000, 1)),
    )
    .await
    .unwrap();
    assert_eq!(operation.status, "completed");
    assert_eq!(hits.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn finite_deadline_bounds_http_retries_and_sleep() {
    for scenario in ["slow_response", "retry_after", "long_sleep"] {
        let server = MockServer::start().await;
        let response = match scenario {
            "slow_response" => ResponseTemplate::new(200)
                .set_body_json(box_info("idle"))
                .set_delay(Duration::from_millis(500)),
            "retry_after" => ResponseTemplate::new(503)
                .insert_header("Retry-After", "1")
                .set_body_json(json!({"ok":false,"code":"unavailable"})),
            _ => ResponseTemplate::new(200).set_body_json(box_info("running")),
        };
        Mock::given(method("GET"))
            .and(path(format!("/sandboxes/{SANDBOX_ID}")))
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;
        let start = Instant::now();
        let result = wait_until_idle_with(
            &client(&server),
            SANDBOX_ID,
            WaitOptions::from_millis(40, 1000),
        )
        .await;
        assert!(
            matches!(result, Err(Error::WaitTimeout { .. })),
            "{scenario}: {result:?}"
        );
        assert!(
            start.elapsed() < Duration::from_millis(400),
            "deadline overrun: {scenario}"
        );
    }
}

#[tokio::test]
async fn zero_timeout_keeps_polling_until_success() {
    let server = MockServer::start().await;
    let hits = Arc::new(AtomicUsize::new(0));
    let seen = hits.clone();
    Mock::given(method("GET"))
        .and(path(format!("/sandboxes/{SANDBOX_ID}")))
        .respond_with(move |_: &Request| {
            ResponseTemplate::new(200).set_body_json(box_info(
                if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                    "running"
                } else {
                    "idle"
                },
            ))
        })
        .expect(2)
        .mount(&server)
        .await;
    assert_eq!(
        wait_until_idle_with(&client(&server), SANDBOX_ID, WaitOptions::from_millis(0, 1))
            .await
            .unwrap()
            .state,
        SandboxState::Idle
    );
}

#[tokio::test]
async fn prompt_failed_is_terminal_and_empty_desktop_url_is_not_ready() {
    let server = MockServer::start().await;
    let mut run = fixture("promptRun");
    run["promptRun"]["status"] = json!("failed");
    run["promptRun"]["done"] = json!(true);
    Mock::given(method("GET"))
        .and(path(format!("/sandboxes/{SANDBOX_ID}/prompts/pr_test-1")))
        .respond_with(ResponseTemplate::new(200).set_body_json(run))
        .mount(&server)
        .await;
    let api = client(&server);
    assert_eq!(
        wait_for_prompt(&api, SANDBOX_ID, "pr_test-1", None)
            .await
            .unwrap()
            .status,
        "failed"
    );
    Mock::given(method("POST"))
        .and(path(format!("/sandboxes/{SANDBOX_ID}/desktop")))
        .and(query_param("vnc", "1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"desktopUrl":"","provisioning":false})),
        )
        .mount(&server)
        .await;
    assert!(matches!(
        wait_for_desktop(
            &api,
            SANDBOX_ID,
            false,
            Some(WaitOptions::from_millis(30, 100))
        )
        .await,
        Err(Error::WaitTimeout { .. })
    ));
}

#[tokio::test]
async fn response_cap_applies_to_success_and_error_bodies() {
    for code in [200, 503] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/snapshots/snap_test-1/files"))
            .respond_with(ResponseTemplate::new(code).set_body_bytes(vec![42; 100]))
            .expect(1)
            .mount(&server)
            .await;
        let api = BoatApi::new(
            BoatClientConfig::new("test")
                .unwrap()
                .with_base_path(server.uri())
                .unwrap()
                .with_max_response_bytes(16),
        )
        .unwrap();
        assert!(matches!(
            api.snapshot_file("snap_test-1", None).await,
            Err(Error::ResponseTooLarge { limit: 16 })
        ));
    }
}

#[tokio::test]
async fn excessive_retry_after_returns_error_without_retrying_early() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/limits"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "3600")
                .set_body_json(json!({"ok":false,"code":"limited"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        client(&server)
            .limits(&Default::default())
            .await
            .unwrap_err()
            .status(),
        Some(429)
    );
}

#[tokio::test]
async fn polling_does_not_restart_transport_after_retry_after_refusal() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/sandboxes/{SANDBOX_ID}")))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "120")
                .set_body_json(json!({"ok":false,"code":"limited"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let result = tokio::time::timeout(
        Duration::from_millis(300),
        wait_until_ready_with(&client(&server), SANDBOX_ID, WaitOptions::from_millis(0, 1)),
    )
    .await
    .unwrap();
    assert_eq!(result.unwrap_err().status(), Some(429));
}
