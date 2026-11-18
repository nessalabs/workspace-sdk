use super::*;
use axum::{routing::post, Json, Router};
use base64::Engine;
use serde_json::{json, Value};

const VM: &str = "bx_23456789";

fn host_key_line(key: &[u8; 32]) -> String {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&11u32.to_be_bytes());
    bytes.extend_from_slice(b"ssh-ed25519");
    bytes.extend_from_slice(&32u32.to_be_bytes());
    bytes.extend_from_slice(key);
    format!(
        "ssh-ed25519 {}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

#[test]
fn only_one_ed25519_host_key_is_pinned() {
    let line = host_key_line(&[7; 32]);
    let encoded = line.split(' ').nth(1).unwrap();
    assert_eq!(ed25519_key(&line), Some(format!("ssh-ed25519 {encoded}")));
    assert_eq!(
        ed25519_key(&format!("{line} comment")),
        Some(format!("ssh-ed25519 {encoded}"))
    );
    let short = format!(
        "ssh-ed25519 {}",
        base64::engine::general_purpose::STANDARD.encode([0u8; 19])
    );
    for bad in [
        "",
        "ssh-ed25519",
        "ssh-ed25519 AAAA",
        short.as_str(),
        "ssh-rsa AAAA",
        "ssh-ed25519 not*base64",
        "ssh-ed25519 AAAA\nevil ssh-ed25519 AAAA",
    ] {
        assert_eq!(ed25519_key(bad), None, "{bad:?}");
    }
    let long = "A".repeat(260);
    assert_eq!(ed25519_key(&format!("ssh-ed25519 {long}")), None);
}

/// A fake Boat answering `sshkey` with `answer`, writing the key it was
/// given to `authorized`. It answers until dropped.
struct Boat {
    api: Api,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Boat {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn boat(answer: Value, authorized: Option<std::path::PathBuf>) -> Boat {
    let router = Router::new().route(
        "/api/v1/sandboxes/{id}/sshkey",
        post(move |Json(request): Json<Value>| {
            if let Some(authorized) = &authorized {
                // Written in place, so a mode a test set is kept.
                let _ = std::fs::OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(authorized)
                    .and_then(|mut file| {
                        std::io::Write::write_all(
                            &mut file,
                            request["key"].as_str().unwrap().as_bytes(),
                        )
                    });
            }
            let answer = answer.clone();
            async move { Json(answer) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api/v1", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let api = Api::new(
        sandbox_client::ClientConfig::new("fixture-token")
            .unwrap()
            .with_base_path(&base)
            .unwrap(),
    )
    .unwrap();
    Boat { api, task }
}

#[tokio::test]
async fn an_authorization_without_everything_to_check_the_machine_is_refused() {
    let good = json!({
        "ok": true, "type": "sandbox.sshkey", "success": true, "sshUser": "user",
        "sshEndpoint": "127.0.0.1:22", "hostKey": host_key_line(&[1; 32]),
    });
    let fake = boat(good.clone(), None).await;
    assert!(authorize(&fake.api, VM, "key".into()).await.is_ok());
    for (field, value) in [
        ("ok", json!(false)),
        ("success", json!(false)),
        ("success", Value::Null),
        ("sshUser", json!("root")),
        ("sshEndpoint", Value::Null),
        ("sshEndpoint", json!("machine.example:22")),
        ("hostKey", Value::Null),
        ("hostKey", json!("ssh-rsa AAAA")),
    ] {
        let mut answer = good.clone();
        answer[field] = value.clone();
        let fake = boat(answer, None).await;
        assert!(
            authorize(&fake.api, VM, "key".into()).await.is_err(),
            "{field}: {value}"
        );
    }
}
