use super::*;
use crate::providers::agent37::ApiKey;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

/// A WebSocket server that echoes binary messages, after checking each
/// connection carries the key; it reports each path it was asked for. Its
/// handshake check answers a refusal by value, as the library asks.
#[allow(clippy::result_large_err)]
async fn echo() -> (String, tokio::sync::mpsc::UnboundedReceiver<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (paths, asked) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let paths = paths.clone();
            tokio::spawn(async move {
                let check = |request: &Request, response: Response| {
                    let key = request.headers().get("X-Agent37-Key");
                    let _ = paths.send(request.uri().path().to_owned());
                    if key.and_then(|key| key.to_str().ok()) == Some("sk_live_fixture") {
                        Ok(response)
                    } else {
                        Err(tokio_tungstenite::tungstenite::http::Response::builder()
                            .status(401)
                            .body(None)
                            .unwrap())
                    }
                };
                let Ok(mut socket) = tokio_tungstenite::accept_hdr_async(stream, check).await
                else {
                    return;
                };
                while let Some(Ok(message)) = socket.next().await {
                    if message.is_binary() && socket.send(message).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    (format!("ws://{address}/{{id}}/ssh"), asked)
}

fn config(tunnel: &str, key: &str) -> Agent37Config {
    Agent37Config::new(ApiKey::new(key).unwrap())
        .with_tunnel(tunnel)
        .unwrap()
}

#[tokio::test]
async fn each_local_connection_is_carried_whole_to_its_instance() {
    let (tunnel, mut asked) = echo().await;
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("tunnel.sock");
    let served = Tunnel::serve(&config(&tunnel, "sk_live_fixture"), "ab12cd34ef", &socket).unwrap();
    for _ in 0..3 {
        let mut local = UnixStream::connect(&socket).await.unwrap();
        let sent: Vec<u8> = (0..100_000u32).map(|i| (i % 253) as u8).collect();
        local.write_all(&sent).await.unwrap();
        let mut back = vec![0u8; sent.len()];
        local.read_exact(&mut back).await.unwrap();
        assert_eq!(back, sent);
        assert_eq!(asked.recv().await.unwrap(), "/ab12cd34ef/ssh");
    }
    // Dropped, it ends what it carries and takes no more.
    let mut open = UnixStream::connect(&socket).await.unwrap();
    open.write_all(b"x").await.unwrap();
    let mut one = [0u8; 1];
    open.read_exact(&mut one).await.unwrap();
    drop(served);
    let mut rest = Vec::new();
    let ended = tokio::time::timeout(Duration::from_secs(5), open.read_to_end(&mut rest)).await;
    assert!(matches!(ended, Ok(Ok(0)) | Ok(Err(_))), "{ended:?}");
}

#[tokio::test]
async fn a_refused_key_closes_the_local_connection() {
    let (tunnel, _asked) = echo().await;
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("tunnel.sock");
    let _served = Tunnel::serve(&config(&tunnel, "sk_live_other"), "ab12cd34ef", &socket).unwrap();
    let mut local = UnixStream::connect(&socket).await.unwrap();
    let mut rest = Vec::new();
    let ended = tokio::time::timeout(Duration::from_secs(5), local.read_to_end(&mut rest))
        .await
        .unwrap();
    assert!(matches!(ended, Ok(0) | Err(_)), "{ended:?}");
    assert!(rest.is_empty());
}

#[test]
fn a_tunnel_address_must_name_its_instance_once() {
    let key = ApiKey::new("sk_live_fixture").unwrap();
    for bad in ["https://{id}.x/", "wss://x/", "wss://{id}/{id}"] {
        assert!(
            Agent37Config::new(key.clone()).with_tunnel(bad).is_err(),
            "{bad}"
        );
    }
    let config = Agent37Config::new(key);
    assert_eq!(
        config.tunnel_to("ab12cd34ef"),
        "wss://ab12cd34ef-22022.agent37.app/"
    );
}
