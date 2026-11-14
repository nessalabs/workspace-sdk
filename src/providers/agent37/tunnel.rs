//! The local end of Agent37's SSH tunnel: a Unix socket in a connection's
//! private directory, each of whose connections is carried over its own
//! WebSocket to the instance's SSH server.
//!
//! ```text
//! ssh ─ProxyCommand nc -U─► tunnel.sock ─► wss://{id}-22022 (X-Agent37-Key) ─► sshd
//! ```
//!
//! The WebSocket's TLS and the workspace key are what make it the
//! workspace's instance; SSH then authenticates the account's key. Each
//! WebSocket carries raw SSH bytes in binary messages.
use super::config::Agent37Config;
use futures_util::{SinkExt, StreamExt};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest, http::HeaderValue, protocol::Message,
};

/// How many connections one tunnel carries at once: a connection's master,
/// its commands, and their files.
const MAX_CARRIED: usize = 64;
/// How long opening one WebSocket may take, waking a sleeping instance
/// included.
const OPEN_TIMEOUT: Duration = Duration::from_secs(60);
/// The largest message a peer may send.
const MAX_MESSAGE: usize = 1024 * 1024;

/// A tunnel being served. Dropping it stops accepting and ends every
/// connection it carries: they are the accepting task's, and end with it.
pub(super) struct Tunnel {
    accepting: tokio::task::JoinHandle<()>,
}
impl Drop for Tunnel {
    fn drop(&mut self) {
        self.accepting.abort();
    }
}

impl Tunnel {
    /// Serve the tunnel to `instance` on `socket`, which must not exist.
    pub(super) fn serve(
        config: &Agent37Config,
        instance: &str,
        socket: &Path,
    ) -> std::io::Result<Self> {
        let listener = UnixListener::bind(socket)?;
        let mut request = config
            .tunnel_to(instance)
            .into_client_request()
            .map_err(std::io::Error::other)?;
        let mut key =
            HeaderValue::from_str(config.key().expose()).map_err(std::io::Error::other)?;
        key.set_sensitive(true);
        request.headers_mut().insert("X-Agent37-Key", key);
        let tls = tls()?;
        let accepting = tokio::spawn(async move {
            // Owned by this task: aborting it drops the set, which aborts
            // every connection still carried.
            let mut carried = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((local, _)) = accepted else { break };
                        // Past the limit a connection is refused, never queued.
                        if carried.len() >= MAX_CARRIED {
                            continue;
                        }
                        carried.spawn(carry(local, request.clone(), tls.clone()));
                    }
                    Some(_) = carried.join_next(), if !carried.is_empty() => {}
                }
            }
        });
        Ok(Self { accepting })
    }
}

/// TLS with the web's roots and the `ring` provider, chosen here so the
/// process's default provider, or its absence, does not matter.
fn tls() -> std::io::Result<tokio_tungstenite::Connector> {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(std::io::Error::other)?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(tokio_tungstenite::Connector::Rustls(Arc::new(config)))
}

/// Carry one local connection until either side ends it.
async fn carry(
    mut local: UnixStream,
    request: tokio_tungstenite::tungstenite::handshake::client::Request,
    tls: tokio_tungstenite::Connector,
) {
    let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE))
        .max_frame_size(Some(MAX_MESSAGE));
    let opened = tokio::time::timeout(
        OPEN_TIMEOUT,
        tokio_tungstenite::connect_async_tls_with_config(request, Some(config), false, Some(tls)),
    )
    .await;
    // A tunnel that does not open ends the local connection, and ssh says
    // the connection was closed.
    let Ok(Ok((remote, _))) = opened else {
        return;
    };
    let (mut to_remote, mut from_remote) = remote.split();
    let (mut from_local, mut to_local) = local.split();
    let up = async {
        let mut buffer = vec![0u8; 32 * 1024];
        loop {
            match from_local.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if to_remote
                        .send(Message::Binary(buffer[..read].to_vec().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
        let _ = to_remote.send(Message::Close(None)).await;
    };
    let down = async {
        while let Some(Ok(message)) = from_remote.next().await {
            let bytes = match message {
                Message::Binary(bytes) => bytes,
                Message::Text(text) => text.as_bytes().to_vec().into(),
                Message::Close(_) => break,
                _ => continue,
            };
            if to_local.write_all(&bytes).await.is_err() {
                break;
            }
        }
        let _ = to_local.shutdown().await;
    };
    // Either direction ending ends the connection: ssh notices a closed
    // socket, and the instance's sshd a closed WebSocket.
    tokio::select! {
        () = up => {}
        () = down => {}
    }
}

#[cfg(test)]
#[path = "tunnel_tests.rs"]
mod tests;
