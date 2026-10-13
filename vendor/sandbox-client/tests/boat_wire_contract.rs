//! Independent Boat v1 wire contracts from the published OpenAPI and SDK 1.9.7.
//! These fixtures intentionally use Boat response keys rather than translating
//! the former Box fixtures along with the client implementation.
use boat_client::{
    wait_until_ready_with, BoatApi, BoatClientConfig, CreateOptions, CreateSandboxRequest, Error,
    SandboxState, SandboxesQuery, WaitOptions,
};
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SANDBOX_ID: &str = "bx_23456789";

fn api(server: &MockServer) -> BoatApi {
    BoatApi::new(
        BoatClientConfig::new("fixture-only")
            .unwrap()
            .with_base_path(format!("{}/api/v1", server.uri()))
            .unwrap()
            .with_org("configured-wallet"),
    )
    .unwrap()
}

fn sandbox(state: &str) -> serde_json::Value {
    json!({
        "id": SANDBOX_ID,
        "name": "contract fixture",
        "state": state,
        "desktopAvailable": false,
        "snapshotAvailable": true,
        "type": "large",
        "vcpu": 8,
        "memoryGB": 16
    })
}

#[test]
fn default_endpoint_matches_boat_openapi_and_published_sdk() {
    let config = BoatClientConfig::new("fixture-only").unwrap();
    assert_eq!(
        BoatClientConfig::DEFAULT_BASE_PATH,
        "https://boat.dev/api/v1"
    );
    assert_eq!(config.base_path, "https://boat.dev/api/v1");
    assert_eq!(
        config.user_agent,
        format!("boat_client/{}", env!("CARGO_PKG_VERSION"))
    );
}

#[tokio::test]
async fn list_uses_boat_scope_and_decodes_sandboxes_with_pagination() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/sandboxes"))
        .and(header("Authorization", "Bearer fixture-only"))
        .and(header("X-Boat-Org", "configured-wallet"))
        .and(query_param("limit", "2"))
        .and(query_param("cursor", "opaque+/="))
        .and(query_param("sort", "asc"))
        .and(query_param("state", "ready,idle"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "type": "sandbox.list",
            "sandboxes": [sandbox("idle")],
            "pageInfo": {"nextCursor": "next+/=", "hasMore": true, "limit": 2}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let response = api(&server)
        .sandboxes(Some(&SandboxesQuery {
            limit: Some(2),
            cursor: Some("opaque+/=".into()),
            sort: Some("asc".into()),
            state: Some("ready,idle".into()),
            ..Default::default()
        }))
        .await
        .unwrap();
    assert!(response.ok);
    assert_eq!(response.type_.as_deref(), Some("sandbox.list"));
    assert_eq!(response.sandboxes.len(), 1);
    assert_eq!(response.sandboxes[0].id, SANDBOX_ID);
    assert_eq!(response.sandboxes[0].state, SandboxState::Idle);
    assert_eq!(response.sandboxes[0].memory_gb, Some(16));
    let page = response.page_info.unwrap();
    assert_eq!(page.next_cursor.as_deref(), Some("next+/="));
    assert!(page.has_more);
    assert_eq!(page.limit, 2);
    let requests = server.received_requests().await.unwrap();
    assert!(requests[0].headers.get("X-Box-Org").is_none());
}

#[tokio::test]
async fn create_keeps_org_scopes_idempotency_and_current_boat_request_options() {
    let server = MockServer::start().await;
    let body = json!({
        "type": "large",
        "ttlSeconds": null,
        "noEnv": true,
        "snapshots": false,
        "monitoring": false,
        "failFast": true,
        "org": "body-wallet"
    });
    Mock::given(method("POST"))
        .and(path("/api/v1/sandboxes"))
        .and(header("Authorization", "Bearer fixture-only"))
        .and(header("X-Boat-Org", "request-wallet"))
        .and(header("Idempotency-Key", "fixture-create-once"))
        .and(query_param("org", "query-wallet"))
        .and(body_json(body.clone()))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({
            "ok": true,
            "type": "sandbox.created",
            "status": "provisioning",
            "ttlSeconds": null,
            "sandbox": sandbox("provisioning")
        })))
        .expect(1)
        .mount(&server)
        .await;

    let request: CreateSandboxRequest = serde_json::from_value(body).unwrap();
    let response = api(&server)
        .create(
            Some(request),
            &CreateOptions {
                org: Some("query-wallet".into()),
                x_boat_org: Some("request-wallet".into()),
                idempotency_key: Some("fixture-create-once".into()),
            },
        )
        .await
        .unwrap();
    assert!(response.ok);
    assert_eq!(response.type_.as_deref(), Some("sandbox.created"));
    assert_eq!(response.status.as_deref(), Some("provisioning"));
    assert_eq!(response.ttl_seconds, None);
    assert_eq!(response.sandbox.id, SANDBOX_ID);
    assert_eq!(response.sandbox.state, SandboxState::Provisioning);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1, "create must not be duplicated");
    assert!(requests[0].headers.get("X-Box-Org").is_none());
}

#[tokio::test]
async fn get_uses_existing_bx_identity_and_the_sandbox_response_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/v1/sandboxes/{SANDBOX_ID}")))
        .and(header("X-Boat-Org", "configured-wallet"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "type": "sandbox.info", "sandbox": sandbox("ready")
        })))
        .expect(1)
        .mount(&server)
        .await;

    let response = api(&server).get(SANDBOX_ID).await.unwrap();
    assert!(response.ok);
    assert_eq!(response.type_.as_deref(), Some("sandbox.info"));
    assert_eq!(response.sandbox.id, SANDBOX_ID);
    assert_eq!(response.sandbox.state, SandboxState::Ready);
    assert_eq!(response.sandbox.type_.as_deref(), Some("large"));
    assert_eq!(response.sandbox.memory_gb, Some(16));
}

#[tokio::test]
async fn minimal_cancelled_response_stops_readiness_without_a_second_request() {
    // Sandbox.state documents this exceptional GET payload as only id/state/error.
    // It is returned once before later GET requests become 404.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/v1/sandboxes/{SANDBOX_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "type": "sandbox.info",
            "sandbox": {
                "id": SANDBOX_ID,
                "state": "cancelled",
                "error": "No machine was available"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let outcome =
        wait_until_ready_with(&api(&server), SANDBOX_ID, WaitOptions::from_millis(500, 1)).await;
    match outcome {
        Err(Error::SandboxTerminal { sandbox_id, state }) => {
            assert_eq!(sandbox_id, SANDBOX_ID);
            assert_eq!(state, "cancelled");
        }
        other => panic!("expected the observed cancellation, got {other:?}"),
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
