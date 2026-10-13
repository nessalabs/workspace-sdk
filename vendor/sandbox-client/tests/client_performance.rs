//! Scoped local HTTP qualification; no credentials, cloud resources or speedup claim.
use boat_client::{BoatApi, BoatClientConfig, SandboxState};
use serde_json::json;
use std::time::{Duration, Instant};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
#[ignore = "explicit scoped performance qualification"]
async fn repeated_sandbox_reads_preserve_payload_and_reuse_the_client() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/sandboxes/bx_23456789"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "sandbox": { "id": "bx_23456789", "name": "fixture", "state": "ready" }
        })))
        .expect(384)
        .mount(&server)
        .await;
    let api = BoatApi::new(
        BoatClientConfig::new("fixture-only")
            .unwrap()
            .with_base_path(server.uri())
            .unwrap(),
    )
    .unwrap();
    let mut elapsed = Vec::new();
    for _ in 0..3 {
        let started = Instant::now();
        tokio::time::timeout(Duration::from_secs(10), async {
            for _ in 0..128 {
                let response = api.get("bx_23456789").await.unwrap();
                assert!(response.ok);
                assert_eq!(response.sandbox.id, "bx_23456789");
                assert_eq!(response.sandbox.name.as_deref(), Some("fixture"));
                assert_eq!(response.sandbox.state, SandboxState::Ready);
            }
        })
        .await
        .expect("bounded read population must finish");
        elapsed.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 384);
    for request in requests {
        assert_eq!(request.headers["authorization"], "Bearer fixture-only");
        assert!(request.body.is_empty());
    }
    println!(
        "{}",
        json!({"scenario":"128 sequential loopback reads per sample", "samples_ms":elapsed,
        "os":std::env::consts::OS,"arch":std::env::consts::ARCH,
        "limitations":"mock server; excludes WAN, cloud, RSS and statistical regression qualification"})
    );
}
