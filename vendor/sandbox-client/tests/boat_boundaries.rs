//! Independent migration boundaries: transport authority, wallet scope, durable identities.
use boat_client::handoff::HandoffReceipt;
use boat_client::{
    BoatApi, BoatClientConfig, CommandRequest, Error, LimitsOptions, SetEnvironmentVarRequest,
};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn remote_http_hosts_cannot_impersonate_loopback_with_a_numeric_prefix() {
    for address in [
        "http://127.evil.example/api/v1",
        "http://127.0.0.1.evil.example/api/v1",
        "http://localhost.evil.example/api/v1",
        "http://192.0.2.1/api/v1",
        "http://[2001:db8::1]/api/v1",
    ] {
        let result = BoatClientConfig::new("fixture-only")
            .unwrap()
            .with_base_path(address);
        assert!(matches!(result, Err(Error::Config(_))), "{address}");
    }
}

#[test]
fn client_construction_validates_a_publicly_modified_api_root() {
    let mut config = BoatClientConfig::new("fixture-only").unwrap();
    config.base_path = "http://127.evil.example/api/v1".into();
    assert!(matches!(BoatApi::new(config), Err(Error::Config(_))));
}

#[test]
fn actual_ipv4_ipv6_and_localhost_loopback_roots_remain_available() {
    for address in [
        "http://localhost:8080/api/v1",
        "http://127.0.0.1:8080/api/v1",
        "http://127.42.0.7:8080/api/v1",
        "http://[::1]:8080/api/v1",
        "https://boat.dev/api/v1",
    ] {
        let config = BoatClientConfig::new("fixture-only")
            .unwrap()
            .with_base_path(address)
            .unwrap_or_else(|error| panic!("{address}: {error}"));
        assert_eq!(config.base_path, address);
        assert!(BoatApi::new(config).is_ok(), "{address}");
    }
}

#[tokio::test]
async fn environments_use_query_scope_while_sandboxes_keep_the_wallet_header() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/environments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "environments": []
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/environments/env-1/vars/FEATURE"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true, "versionId": "version-2", "versionNumber": 2
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/sandboxes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "type": "sandbox.list", "sandboxes": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    let org = "Operations & R&D/+?";
    let api = BoatApi::new(
        BoatClientConfig::new("fixture-only")
            .unwrap()
            .with_base_path(server.uri())
            .unwrap()
            .with_org(org),
    )
    .unwrap();
    assert!(api.environments().await.unwrap().environments.is_empty());
    assert!(
        api.set_environment_var(
            "env-1",
            "FEATURE",
            SetEnvironmentVarRequest {
                value: "unchanged".into(),
            },
        )
        .await
        .unwrap()
        .success
    );
    assert!(api.sandboxes(None).await.unwrap().sandboxes.is_empty());

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    for request in &requests[..2] {
        let query: Vec<_> = request.url.query_pairs().collect();
        assert_eq!(query, vec![("org".into(), org.into())]);
        assert!(!request.headers.contains_key("X-Boat-Org"));
        assert!(!request.headers.contains_key("X-Box-Org"));
        assert_eq!(request.headers["Authorization"], "Bearer fixture-only");
    }
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&requests[1].body).unwrap(),
        json!({"value": "unchanged"})
    );
    assert_eq!(requests[2].headers["X-Boat-Org"], org);
    assert!(!requests[2].headers.contains_key("X-Box-Org"));
    assert!(requests[2].url.query().is_none());
}

#[test]
fn current_handoff_receipts_use_sandbox_identity_fields_and_reject_old_only_names() {
    let persisted = json!({
        "schema_version": 1,
        "id": "existing-transfer",
        "revision": 0,
        "plan": {
            "source_sandbox_id": "bx_23456789",
            "target_sandbox_id": "bx_abcdefgh",
            "intent": "immutable-work-manifest"
        },
        "phase": "prepared",
        "checkpoint": null,
        "run_id": null,
        "last_reconciliation": null
    });
    let receipt: HandoffReceipt = serde_json::from_value(persisted.clone()).unwrap();
    assert_eq!(receipt.plan().source_sandbox_id, "bx_23456789");
    assert_eq!(receipt.plan().target_sandbox_id, "bx_abcdefgh");
    assert_eq!(receipt.plan().intent, "immutable-work-manifest");
    assert_eq!(serde_json::to_value(receipt).unwrap(), persisted);

    for replacements in [
        vec![("source_sandbox_id", "source_box_id")],
        vec![("target_sandbox_id", "target_box_id")],
        vec![
            ("source_sandbox_id", "source_box_id"),
            ("target_sandbox_id", "target_box_id"),
        ],
    ] {
        let mut obsolete = persisted.clone();
        let plan = obsolete["plan"].as_object_mut().unwrap();
        for (current, old) in replacements {
            let identity = plan.remove(current).unwrap();
            plan.insert(old.into(), identity);
        }
        assert!(
            serde_json::from_value::<HandoffReceipt>(obsolete).is_err(),
            "old names must not substitute for required current identities"
        );
    }
}

#[tokio::test]
async fn cleared_scope_omits_wallet_selection_and_personal_scope_is_explicit() {
    let server = MockServer::start().await;
    for (route, body) in [
        ("/sandboxes", json!({"ok": true, "sandboxes": []})),
        (
            "/limits",
            json!({
                "ok": true, "type": "limits", "canStart": true,
                "activeSandboxes": 0, "maxActiveSandboxes": 5,
                "billingStatus": "active"
            }),
        ),
        ("/environments", json!({"environments": []})),
    ] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(2)
            .mount(&server)
            .await;
    }
    let configured = BoatApi::new(
        BoatClientConfig::new("fixture-only")
            .unwrap()
            .with_base_path(server.uri())
            .unwrap()
            .with_org("team-wallet"),
    )
    .unwrap();
    for api in [
        configured.clone().with_org(None),
        configured.clone().with_org(Some("personal".into())),
    ] {
        api.sandboxes(None).await.unwrap();
        api.limits(&LimitsOptions::default()).await.unwrap();
        api.environments().await.unwrap();
    }
    assert_eq!(configured.config().org.as_deref(), Some("team-wallet"));
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 6);
    for request in &requests[..3] {
        assert!(!request.headers.contains_key("X-Boat-Org"));
        assert!(request.url.query().is_none());
    }
    for request in &requests[3..5] {
        assert_eq!(request.headers["X-Boat-Org"], "personal");
        assert!(request.url.query().is_none());
    }
    assert!(!requests[5].headers.contains_key("X-Boat-Org"));
    assert_eq!(requests[5].url.query(), Some("org=personal"));
}

#[tokio::test]
async fn json_command_methods_reject_streaming_before_executing_remote_work() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/sandboxes/bx_23456789/commands"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "{\"type\":\"started\"}\n{\"type\":\"stdout\",\"data\":\"done\"}\n{\"type\":\"exit\",\"exitCode\":0}\n",
            "application/x-ndjson",
        ))
        .expect(0)
        .mount(&server)
        .await;
    let api = BoatApi::new(
        BoatClientConfig::new("fixture-only")
            .unwrap()
            .with_base_path(server.uri())
            .unwrap(),
    )
    .unwrap();
    let mut request = CommandRequest::new("printf done");
    request.stream = Some(true);
    assert!(matches!(
        api.command("bx_23456789", request.clone()).await,
        Err(Error::Config(_))
    ));
    assert!(matches!(
        api.command_raw("bx_23456789", request).await,
        Err(Error::Config(_))
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
}
