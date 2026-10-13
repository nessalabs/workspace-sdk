mod common;
use boat_client::*;
use common::{logs::Logs, sdk_case};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tracing::instrument::WithSubscriber;
use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};

fn config(server: &MockServer, attempts: u32) -> BoatClientConfig {
    BoatClientConfig::new("secret-fixture-token")
        .unwrap()
        .with_base_path(server.uri())
        .unwrap()
        .with_retry(RetryConfig {
            max_attempts: attempts,
            initial_delay: Duration::ZERO,
            jitter: false,
            ..Default::default()
        })
}

#[tokio::test]
async fn configurable_attempts_retry_to_success_and_can_be_disabled() {
    for attempts in [1, 5] {
        let server = MockServer::start().await;
        let count = Arc::new(AtomicUsize::new(0));
        let requests = count.clone();
        Mock::given(method("GET"))
            .respond_with(move |_: &wiremock::Request| {
                if requests.fetch_add(1, Ordering::SeqCst) < 3 {
                    ResponseTemplate::new(503)
                } else {
                    ResponseTemplate::new(200).set_body_json(&sdk_case("apiKeys")["response"])
                }
            })
            .expect(if attempts == 1 { 1 } else { 4 })
            .mount(&server)
            .await;
        let api = BoatApi::new(config(&server, attempts)).unwrap();
        let result = api.api_keys().await;
        assert_eq!(result.is_ok(), attempts > 1);
        assert_eq!(api.config().retry.max_attempts, attempts);
    }
}

#[tokio::test]
async fn retry_after_survives_exhaustion_and_mutations_are_not_replayed() {
    for (verb, body) in [
        (
            "GET",
            serde_json::json!({"error":{"code":"limited","message":"secret-body"}}),
        ),
        ("POST", serde_json::json!({"problem":"secret-body"})),
    ] {
        let server = MockServer::start().await;
        Mock::given(method(verb))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("Retry-After", "120")
                    .set_body_json(body),
            )
            .expect(1)
            .mount(&server)
            .await;
        let api = BoatApi::new(config(&server, 9)).unwrap();
        let error = if verb == "GET" {
            api.api_keys().await.unwrap_err()
        } else {
            api.create(Some(CreateSandboxRequest::ttl(300)), &Default::default())
                .await
                .unwrap_err()
        };
        assert_eq!(error.retry_after(), Some(Duration::from_secs(120)));
        assert!(error.is_retryable());
        assert!(!format!("{error:?}").contains("secret-body"));
    }
}

#[tokio::test]
async fn retry_after_date_and_invalid_header_are_exposed_correctly() {
    for header in [
        httpdate::fmt_http_date(std::time::SystemTime::now() + Duration::from_secs(90)),
        "invalid".into(),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", header.as_str()))
            .expect(1)
            .mount(&server)
            .await;
        let error = BoatApi::new(config(&server, 1))
            .unwrap()
            .api_keys()
            .await
            .unwrap_err();
        if header == "invalid" {
            assert_eq!(error.retry_after(), None);
        } else {
            assert!((Duration::from_secs(85)..=Duration::from_secs(90))
                .contains(&error.retry_after().unwrap()));
        }
    }
}

#[tokio::test]
async fn subscriber_filters_control_http_and_retry_logs_without_sensitive_data() {
    for filter in [
        "boat_client=off",
        "boat_client=info",
        "boat_client=debug",
        "boat_client=trace",
    ] {
        if common::logs::isolated(
            "subscriber_filters_control_http_and_retry_logs_without_sensitive_data",
            filter,
        ) {
            continue;
        }
        let server = MockServer::start().await;
        Mock::given(method("GET")).respond_with(ResponseTemplate::new(503).set_body_json(serde_json::json!({
            "error":{"code":"secret-code", "message":"secret-message", "details":{"value":"secret-details"}}, "requestId":"secret-request-id"
        }))).expect(3).mount(&server).await;
        let logs = Logs::default();
        let api = BoatApi::new(config(&server, 3).with_org("secret-org")).unwrap();
        api.api_key_usage("secret-key-id")
            .with_subscriber(logs.subscriber())
            .await
            .unwrap_err();
        let output = logs.text();
        let enabled = filter.ends_with("debug") || filter.ends_with("trace");
        assert_eq!(
            output.matches("retrying GET request").count(),
            if enabled { 2 } else { 0 }
        );
        if enabled {
            for expected in [
                "api_key_usage",
                "method=GET",
                "attempt=1",
                "next_attempt=2",
                "max_attempts=3",
                "status=Some(503)",
                "delay_ms=0",
                "elapsed_ms=",
                "request failed",
            ] {
                assert!(output.contains(expected), "missing {expected}: {output}");
            }
            assert_eq!(output.matches("request attempt finished").count(), 3);
        } else {
            assert!(output.is_empty());
        }
        assert!(!output.contains("secret-"));
        assert!(!output.contains(&server.uri()));
    }
}

#[tokio::test]
async fn transport_logs_success_decode_failure_and_mutation_safely() {
    if common::logs::isolated(
        "transport_logs_success_decode_failure_and_mutation_safely",
        "boat_client=trace",
    ) {
        return;
    }
    for (operation, method_name, body, success) in [
        (
            "secrets",
            "GET",
            sdk_case("secrets")["response"].clone(),
            true,
        ),
        (
            "api_keys",
            "GET",
            serde_json::json!({"ok":"secret-invalid-value"}),
            false,
        ),
        (
            "write_file",
            "PUT",
            sdk_case("writeFile")["response"].clone(),
            true,
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method(method_name))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(&server)
            .await;
        let logs = Logs::default();
        let api = BoatApi::new(config(&server, 3)).unwrap();
        let run = async {
            match operation {
                "secrets" => api.secrets().await.map(|_| ()),
                "api_keys" => api.api_keys().await.map(|_| ()),
                _ => api
                    .write_file(
                        common::SANDBOX_ID,
                        FileWriteRequest {
                            path: "/tmp/secret-path".into(),
                            content: "secret-content".into(),
                            encoding: Some("utf8".into()),
                        },
                    )
                    .await
                    .map(|_| ()),
            }
        };
        assert_eq!(
            run.with_subscriber(logs.subscriber()).await.is_ok(),
            success
        );
        let output = logs.text();
        assert!(output.contains(operation));
        assert!(output.contains("status=Some(200)"));
        assert!(output.contains(if success {
            "request completed"
        } else {
            "request failed"
        }));
        if !success {
            assert!(output.contains("error_kind=\"decode\""));
        }
        assert!(!output.contains("secret-"));
        assert!(!output.contains(common::SANDBOX_ID));
        assert!(!output.contains(&server.uri()));
    }
}
