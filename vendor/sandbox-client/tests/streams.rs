mod common;
use boat_client::*;
use common::*;
use futures_util::{pin_mut, StreamExt, TryStreamExt};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, Request, ResponseTemplate,
};

fn event(id: &str, task: &str, status: Option<&str>) -> Value {
    json!({"id":id,"timestamp":1000,"taskId":task,"type":if status.is_some(){"prompt"}else{"response"},"data":{"status":status}})
}
fn page(events: Vec<Value>, cursor: Option<&str>, more: bool) -> Value {
    json!({"ok":true,"id":SANDBOX_ID,"events":events,"pageInfo":{"limit":100,"nextCursor":cursor,"hasMore":more}})
}
async fn pages(server: &MockServer, responses: Vec<Value>) {
    let count = responses.len() as u64;
    let index = Arc::new(AtomicUsize::new(0));
    Mock::given(method("GET"))
        .and(path(format!("/sandboxes/{SANDBOX_ID}/events")))
        .respond_with(move |_: &Request| {
            let i = index.fetch_add(1, Ordering::SeqCst);
            ResponseTemplate::new(200)
                .set_body_json(responses.get(i).expect("unexpected extra event poll"))
        })
        .expect(count)
        .mount(server)
        .await;
}
async fn queue_prompt(server: &MockServer) {
    let mut response = sdk_case("prompt")["response"].clone();
    response["promptId"] = json!("mine");
    Mock::given(method("POST"))
        .and(path(format!("/sandboxes/{SANDBOX_ID}/prompt")))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(server)
        .await;
}
async fn status(server: &MockServer, state: &str) {
    let mut response = sdk_case("promptRunStatus")["response"].clone();
    response["promptRun"]["status"] = json!(state);
    Mock::given(method("GET"))
        .and(path(format!("/sandboxes/{SANDBOX_ID}/prompts/mine")))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(server)
        .await;
}
fn prompt_request() -> PromptRequest {
    PromptRequest::new(PromptProvider::Codex, "fixture prompt")
}
fn events_options() -> StreamEventsOptions {
    StreamEventsOptions {
        include_existing: true,
        wait: WaitOptions::from_millis(1000, 1),
        ..Default::default()
    }
}
fn prompt_options() -> Option<StreamPromptOptions> {
    Some(StreamPromptOptions {
        wait: WaitOptions::from_millis(1000, 1),
        ..Default::default()
    })
}
fn query(request: &Request, key: &str) -> Option<String> {
    request
        .url
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

#[tokio::test]
async fn streams_are_lazy_and_dropping_them_starts_no_work() {
    let server = MockServer::start().await;
    let api = client(&server);
    drop(stream_events(&api, SANDBOX_ID, None));
    drop(stream_prompt(&api, SANDBOX_ID, prompt_request(), None));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn latest_bootstrap_skips_large_history_and_encodes_cursor() {
    let server = MockServer::start().await;
    pages(
        &server,
        vec![
            page(
                vec![event("old-500", "other", None)],
                Some("wrong-desc-cursor"),
                true,
            ),
            page(vec![event("new-501", "mine", None)], Some("next"), false),
        ],
    )
    .await;
    let api = client(&server);
    let stream = stream_events(
        &api,
        SANDBOX_ID,
        Some(StreamEventsOptions {
            type_: Some("response".into()),
            ..Default::default()
        }),
    );
    pin_mut!(stream);
    assert_eq!(
        stream.next().await.unwrap().unwrap().id.as_deref(),
        Some("new-501")
    );
    let requests = server.received_requests().await.unwrap();
    assert_eq!(query(&requests[0], "sort").as_deref(), Some("desc"));
    assert_eq!(query(&requests[0], "limit").as_deref(), Some("1"));
    assert_eq!(query(&requests[1], "sort").as_deref(), Some("asc"));
    // base64url("1000:old-500"), with no padding.
    assert_eq!(
        query(&requests[1], "cursor").as_deref(),
        Some("MTAwMDpvbGQtNTAw")
    );
    assert!(requests
        .iter()
        .all(|r| query(r, "type").as_deref() == Some("response")));
}

#[tokio::test]
async fn include_existing_drains_pages_without_sleeping() {
    let server = MockServer::start().await;
    pages(
        &server,
        vec![
            page(vec![event("1", "mine", None)], Some("first/+="), true),
            page(vec![event("2", "mine", None)], Some("second"), false),
        ],
    )
    .await;
    let api = client(&server);
    let options = StreamEventsOptions {
        wait: WaitOptions::from_millis(500, 60_000),
        ..events_options()
    };
    let results: Vec<_> = stream_events(&api, SANDBOX_ID, Some(options))
        .take(2)
        .try_collect()
        .await
        .unwrap();
    assert_eq!(results.len(), 2);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(query(&requests[0], "cursor"), None);
    assert_eq!(query(&requests[1], "cursor").as_deref(), Some("first/+="));
}

#[tokio::test]
async fn terminal_prompt_event_drains_all_pages_and_filters_other_tasks() {
    let server = MockServer::start().await;
    pages(
        &server,
        vec![
            page(vec![], None, false),
            page(
                vec![
                    event("1", "mine", Some("finished")),
                    event("2", "other", None),
                ],
                Some("other-task-cursor"),
                true,
            ),
            page(vec![event("3", "mine", None)], Some("final"), false),
        ],
    )
    .await;
    queue_prompt(&server).await;
    let api = client(&server);
    let results: Vec<_> = stream_prompt(&api, SANDBOX_ID, prompt_request(), prompt_options())
        .try_collect()
        .await
        .unwrap();
    assert_eq!(
        results
            .iter()
            .map(|e| e.id.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["1", "3"]
    );
    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        query(requests.last().unwrap(), "cursor").as_deref(),
        Some("other-task-cursor")
    );
    assert!(requests
        .iter()
        .filter(|r| query(r, "sort").as_deref() == Some("asc"))
        .all(|r| query(r, "type").as_deref() == Some("prompt,response,usage_limit,shield")));
}

#[tokio::test]
async fn terminal_status_refreshes_and_drains_final_events() {
    for state in ["finished", "failed"] {
        let server = MockServer::start().await;
        pages(
            &server,
            vec![
                page(vec![], None, false),
                page(vec![event("other", "other", None)], Some("other"), false),
                page(vec![event("late", "mine", None)], Some("late"), true),
                page(vec![event("last", "mine", None)], Some("last"), false),
            ],
        )
        .await;
        queue_prompt(&server).await;
        status(&server, state).await;
        let api = client(&server);
        let results: Vec<_> = stream_prompt(&api, SANDBOX_ID, prompt_request(), prompt_options())
            .try_collect()
            .await
            .unwrap();
        assert_eq!(
            results
                .iter()
                .map(|e| e.id.as_deref().unwrap())
                .collect::<Vec<_>>(),
            ["late", "last"]
        );
    }
}

#[tokio::test]
async fn cancellation_before_first_poll_sends_no_prompt_or_request() {
    let server = MockServer::start().await;
    let api = client(&server);
    let token = CancellationToken::new();
    token.cancel();
    let stream = stream_prompt(
        &api,
        SANDBOX_ID,
        prompt_request(),
        Some(StreamPromptOptions {
            cancellation: Some(token),
            ..Default::default()
        }),
    );
    pin_mut!(stream);
    assert!(matches!(stream.next().await, Some(Err(Error::Aborted))));
    assert!(stream.next().await.is_none());
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn cancellation_interrupts_inflight_http() {
    let server = MockServer::start().await;
    let api = client(&server);
    let notified = Arc::new(tokio::sync::Notify::new());
    let observed = notified.clone();
    Mock::given(method("GET"))
        .respond_with(move |_: &Request| {
            observed.notify_one();
            ResponseTemplate::new(200)
                .set_body_json(page(vec![], None, false))
                .set_delay(Duration::from_secs(10))
        })
        .expect(1)
        .mount(&server)
        .await;
    let token = CancellationToken::new();
    let cancel = token.clone();
    let stream = stream_events(
        &api,
        SANDBOX_ID,
        Some(StreamEventsOptions {
            cancellation: Some(token),
            ..events_options()
        }),
    );
    pin_mut!(stream);
    let (result, ()) = tokio::join!(stream.next(), async {
        notified.notified().await;
        cancel.cancel()
    });
    assert!(matches!(result, Some(Err(Error::Aborted))));
}

#[tokio::test]
async fn cancellation_interrupts_sleep_and_buffered_events() {
    let server = MockServer::start().await;
    let api = client(&server);
    pages(
        &server,
        vec![page(
            vec![event("1", "mine", None), event("2", "mine", None)],
            Some("next"),
            false,
        )],
    )
    .await;
    let token = CancellationToken::new();
    let stream = stream_events(
        &api,
        SANDBOX_ID,
        Some(StreamEventsOptions {
            cancellation: Some(token.clone()),
            ..events_options()
        }),
    );
    pin_mut!(stream);
    stream.next().await.unwrap().unwrap();
    token.cancel();
    assert!(matches!(stream.next().await, Some(Err(Error::Aborted))));

    let server = MockServer::start().await;
    let api = client(&server);
    pages(
        &server,
        vec![page(vec![event("1", "mine", None)], Some("next"), false)],
    )
    .await;
    let token = CancellationToken::new();
    let stream = stream_events(
        &api,
        SANDBOX_ID,
        Some(StreamEventsOptions {
            cancellation: Some(token.clone()),
            wait: WaitOptions::from_millis(0, 60_000),
            ..events_options()
        }),
    );
    pin_mut!(stream);
    stream.next().await.unwrap().unwrap();
    let (result, ()) = tokio::join!(stream.next(), async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        token.cancel()
    });
    assert!(matches!(result, Some(Err(Error::Aborted))));
}

#[tokio::test]
async fn deadline_covers_http_sleep_and_buffered_pages() {
    for scenario in ["http", "sleep", "buffer"] {
        let server = MockServer::start().await;
        let api = client(&server);
        let response = ResponseTemplate::new(200).set_body_json(page(
            vec![event("1", "mine", None), event("2", "mine", None)],
            Some("next"),
            false,
        ));
        Mock::given(method("GET"))
            .respond_with(if scenario == "http" {
                response.set_delay(Duration::from_secs(10))
            } else {
                response
            })
            .expect(1)
            .mount(&server)
            .await;
        let stream = stream_events(
            &api,
            SANDBOX_ID,
            Some(StreamEventsOptions {
                wait: WaitOptions::from_millis(30, 60_000),
                ..events_options()
            }),
        );
        pin_mut!(stream);
        if scenario != "http" {
            stream.next().await.unwrap().unwrap();
            if scenario == "sleep" {
                stream.next().await.unwrap().unwrap();
            } else {
                tokio::time::sleep(Duration::from_millis(40)).await;
            }
        }
        assert!(
            matches!(stream.next().await, Some(Err(Error::WaitTimeout { .. }))),
            "{scenario}"
        );
        assert!(stream.next().await.is_none());
    }
}

#[tokio::test]
async fn repeated_cursor_fails_before_yielding_duplicates() {
    let server = MockServer::start().await;
    let api = client(&server);
    let repeated = page(vec![event("same", "mine", None)], Some("same"), true);
    pages(&server, vec![repeated.clone(), repeated]).await;
    let stream = stream_events(&api, SANDBOX_ID, Some(events_options()));
    pin_mut!(stream);
    stream.next().await.unwrap().unwrap();
    assert!(matches!(
        stream.next().await,
        Some(Err(Error::Unexpected(_)))
    ));
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn missing_cursor_cannot_loop_or_replay() {
    for body in [
        page(vec![json!({"type":"response"})], None, false),
        page(vec![], None, true),
    ] {
        let server = MockServer::start().await;
        let api = client(&server);
        pages(&server, vec![body]).await;
        let stream = stream_events(&api, SANDBOX_ID, Some(events_options()));
        pin_mut!(stream);
        assert!(matches!(
            stream.next().await,
            Some(Err(Error::Unexpected(_)))
        ));
    }
}

#[tokio::test]
async fn invalid_limit_fails_before_network() {
    let server = MockServer::start().await;
    let api = client(&server);
    let stream = stream_events(
        &api,
        SANDBOX_ID,
        Some(StreamEventsOptions {
            limit: 0,
            ..Default::default()
        }),
    );
    pin_mut!(stream);
    assert!(matches!(stream.next().await, Some(Err(Error::Config(_)))));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn dropping_stream_does_not_prefetch() {
    let server = MockServer::start().await;
    let api = client(&server);
    pages(
        &server,
        vec![page(vec![event("1", "mine", None)], Some("next"), true)],
    )
    .await;
    let stream = stream_events(&api, SANDBOX_ID, Some(events_options()));
    let first = stream.take(1).try_collect::<Vec<_>>().await.unwrap();
    assert_eq!(first.len(), 1);
    tokio::task::yield_now().await;
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
