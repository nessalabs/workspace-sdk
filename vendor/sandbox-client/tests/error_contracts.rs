mod common;
use boat_client::*;
use common::*;
use serde_json::{json, Value};
use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn error_envelopes_preserve_details_without_logging_them() {
    for body in [
        json!({"ok":false,"code":"bad","message":"secret-text","details":{"secret":"secret-detail"}}),
        json!({"ok":false,"error":{"code":"bad","message":"secret-text","details":{"secret":"secret-detail"}}}),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(422).set_body_json(body))
            .expect(1)
            .mount(&server)
            .await;
        let error = client(&server).api_keys().await.unwrap_err();
        assert_eq!(error.status(), Some(422));
        assert_eq!(error.api_message(), Some("secret-text"));
        assert_eq!(
            error.api_details(),
            Some(&json!({"secret":"secret-detail"}))
        );
        assert!(!format!("{error} {error:?}").contains("secret-"));
    }
}

#[tokio::test]
async fn non_json_transient_errors_retain_status_and_use_three_attempts() {
    for code in [429, 502, 503, 504] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(code)
                    .insert_header("Retry-After", "0")
                    .set_body_string("<html>secret-body</html>"),
            )
            .expect(3)
            .mount(&server)
            .await;
        let error = client(&server).list_webhooks().await.unwrap_err();
        assert!(matches!(error, Error::HttpStatus { .. }));
        assert_eq!(error.status(), Some(code));
        assert!(error.is_retryable());
        assert_eq!(error.unexpected_body(), Some("<html>secret-body</html>"));
        assert!(!format!("{error} {error:?}").contains("secret-body"));
    }
}

#[tokio::test]
async fn non_envelope_errors_do_not_retry_other_statuses() {
    for code in [400, 401, 403, 404, 409, 422, 500] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(code).set_body_json(json!({"problem":"secret-body"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let error = client(&server).secrets().await.unwrap_err();
        assert!(matches!(error, Error::HttpStatus { .. }));
        assert_eq!(error.status(), Some(code));
        assert!(!error.is_retryable());
    }
}

#[tokio::test]
async fn decoding_errors_hide_invalid_values_and_are_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":"secret-invalid-bool"})))
        .expect(1)
        .mount(&server)
        .await;
    let error = client(&server).api_keys().await.unwrap_err();
    assert!(matches!(error, Error::Serde(_)));
    assert!(!error.is_retryable());
    assert!(!format!("{error} {error:?}").contains("secret-invalid-bool"));
}

#[tokio::test]
async fn transport_errors_strip_private_urls() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let api = BoatApi::new(
        BoatClientConfig::new("fixture-only")
            .unwrap()
            .with_base_path(format!("http://{address}/secret-url-segment"))
            .unwrap(),
    )
    .unwrap();
    let error = api.api_keys().await.unwrap_err();
    assert!(matches!(error, Error::Http(_)));
    assert!(!format!("{error} {error:?}").contains("secret-url-segment"));
    if let Error::Http(inner) = error {
        assert!(inner.url().is_none());
    }
}

#[tokio::test]
async fn successful_http_preserves_ok_false_as_sdk_does() {
    let server = MockServer::start().await;
    let mut response = sdk_case("apiKeys")["response"].clone();
    response["ok"] = json!(false);
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;
    assert!(!client(&server).api_keys().await.unwrap().ok);
}

#[tokio::test]
async fn chunked_response_cap_does_not_require_content_length() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let serve = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 1024];
        let _ = socket.read(&mut buffer).await.unwrap();
        socket.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n10\r\n0123456789abcdef\r\n10\r\n0123456789abcdef\r\n0\r\n\r\n").await.unwrap();
    });
    let api = BoatApi::new(
        BoatClientConfig::new("fixture-only")
            .unwrap()
            .with_base_path(format!("http://{address}"))
            .unwrap()
            .with_max_response_bytes(20),
    )
    .unwrap();
    assert!(matches!(
        api.artifact(SANDBOX_ID, "fixture.bin").await,
        Err(Error::ResponseTooLarge { limit: 20 })
    ));
    serve.await.unwrap();
}

#[test]
fn nullable_requests_preserve_omission_null_and_value() {
    fn check<T: serde::de::DeserializeOwned + serde::Serialize>(field: &str, value: Value) {
        for original in [json!({}), json!({field:null}), json!({field:value})] {
            let request: T = serde_json::from_value(original.clone()).unwrap();
            assert_eq!(serde_json::to_value(request).unwrap(), original);
        }
    }
    check::<CreateSandboxRequest>("ttlSeconds", json!(300));
    check::<UpdateSandboxRequest>("ttlSeconds", json!(300));
    check::<ResumeRequest>("ttlSeconds", json!(300));
    check::<ForkRequest>("ttlSeconds", json!(300));
    check::<WebhookUpdateRequest>("name", json!("fixture-name"));
}

#[test]
fn limits_preserve_fractional_and_negative_balances() {
    let mut body = sdk_case("limits")["response"].clone();
    for field in [
        "creditBalanceSeconds",
        "subscriptionQuotaSeconds",
        "subscriptionRemainingSeconds",
        "packBalanceSeconds",
        "creditPurchasedSeconds",
        "creditUsedSeconds",
        "liveUsageSeconds",
        "creditSecondsPerDollar",
    ] {
        body[field] = json!(-0.125);
    }
    let response: LimitsResponse = serde_json::from_value(body).unwrap();
    assert_eq!(response.credit_balance_seconds, Some(-0.125));
    assert_eq!(response.live_usage_seconds, Some(-0.125));
}

#[tokio::test]
async fn prompt_nullable_options_send_explicit_null() {
    let server = MockServer::start().await;
    let body =
        json!({"provider":"codex","prompt":"fixture prompt","model":null,"reasoningEffort":null});
    Mock::given(method("POST"))
        .and(wiremock::matchers::body_json(&body))
        .respond_with(ResponseTemplate::new(200).set_body_json(&sdk_case("prompt")["response"]))
        .expect(1)
        .mount(&server)
        .await;
    let request: PromptRequest = serde_json::from_value(body.clone()).unwrap();
    assert_eq!(request.model, Some(None));
    assert_eq!(request.reasoning_effort, Some(None));
    assert_eq!(serde_json::to_value(&request).unwrap(), body);
    client(&server).prompt(SANDBOX_ID, request).await.unwrap();
}
