//! An SSH agent served from this process, so a connection's private key is
//! never written to disk.
//!
//! ```text
//! connect:  KeyPair ─► Signer (seed held in memory, wiped when dropped)
//!           ─► Agent listening on <connection directory>/agent
//! ssh:      -i <directory>/key.pub, IdentityAgent=<directory>/agent
//!           ─► REQUEST_IDENTITIES ─► the one public key
//!           ─► SIGN_REQUEST(that key, data) ─► ed25519 signature
//! drop:     the agent's tasks are aborted with the connection
//! ```
//!
//! Only this account's processes can reach the socket: it lives in the
//! connection's directory, which only this account may enter. The
//! agent answers just the two requests ssh needs to sign in, for its one
//! key, and fails everything else. A process killed outright leaves the
//! socket file behind, never the key.
use super::keys::{public_blob, put_string, take_string, KeyPair};
use crate::Result;
use ed25519_dalek::{Signer as _, SigningKey};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    task::JoinHandle,
};

const FAILURE: u8 = 5;
const REQUEST_IDENTITIES: u8 = 11;
const IDENTITIES_ANSWER: u8 = 12;
const SIGN_REQUEST: u8 = 13;
const SIGN_RESPONSE: u8 = 14;
/// The largest request read: ssh's sign requests are a few hundred bytes.
const MAX_REQUEST: usize = 64 * 1024;
/// The most clients served at once, so stuck ones cannot pile up tasks.
const MAX_CLIENTS: usize = 32;
/// How long a client may take to send a whole request, or to start the
/// next one: ssh asks at once, so a silent client is dropped.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const KEY_TYPE: &[u8] = b"ssh-ed25519";

/// An ed25519 key that signs in memory.
pub(super) struct Signer {
    key: SigningKey,
    /// The public key in SSH's wire form.
    blob: Vec<u8>,
}

impl Signer {
    /// The signer for `pair`.
    pub(super) fn new(pair: &KeyPair) -> Self {
        let key = pair.signing_key();
        let blob = public_blob(&key);
        Self { key, blob }
    }

    /// The answer to one request.
    fn answer(&self, request: &[u8]) -> Vec<u8> {
        let mut reply = Vec::new();
        match request.split_first() {
            Some((&REQUEST_IDENTITIES, [])) => {
                reply.push(IDENTITIES_ANSWER);
                reply.extend_from_slice(&1u32.to_be_bytes());
                put_string(&mut reply, &self.blob);
                put_string(&mut reply, b"workspace-sdk");
            }
            Some((&SIGN_REQUEST, mut body)) => {
                let blob = take_string(&mut body);
                let data = take_string(&mut body);
                // The flags only choose RSA hashes; ed25519 has one form.
                let flags = body.len() == 4;
                match (blob, data, flags) {
                    (Some(blob), Some(data), true) if blob == self.blob => {
                        let signature = self.key.sign(data);
                        let mut inner = Vec::with_capacity(83);
                        put_string(&mut inner, KEY_TYPE);
                        put_string(&mut inner, &signature.to_bytes());
                        reply.push(SIGN_RESPONSE);
                        put_string(&mut reply, &inner);
                    }
                    _ => reply.push(FAILURE),
                }
            }
            _ => reply.push(FAILURE),
        }
        reply
    }
}

/// An agent serving one key on a Unix socket until dropped.
pub(super) struct Agent {
    listener: JoinHandle<()>,
}

impl Agent {
    /// Serve `signer` on a new socket at `socket`, until dropped. It must
    /// be called within a tokio runtime, whose task serves it.
    ///
    /// # Errors
    /// The socket's own, when it cannot be bound.
    pub(super) fn serve(signer: Signer, socket: &Path) -> Result<Self> {
        let listener = UnixListener::bind(socket)?;
        let signer = Arc::new(signer);
        let listener = tokio::spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else {
                            // Out of descriptors, say: wait rather than spin.
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            continue;
                        };
                        // Too many at once: this one is closed unanswered.
                        if clients.len() < MAX_CLIENTS {
                            let signer = signer.clone();
                            clients.spawn(async move {
                                let _ = serve_client(stream, &signer).await;
                            });
                        }
                    }
                    Some(_) = clients.join_next(), if !clients.is_empty() => {}
                }
            }
            // Dropping the set, when this task is aborted, aborts every client.
        });
        Ok(Self { listener })
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        self.listener.abort();
    }
}

/// Answer `stream`'s requests until it ends, sends one too large, or is
/// silent, or leaves a reply unread, for [`REQUEST_TIMEOUT`].
async fn serve_client(mut stream: UnixStream, signer: &Signer) -> std::io::Result<()> {
    loop {
        let reply = match tokio::time::timeout(REQUEST_TIMEOUT, read_request(&mut stream)).await {
            Ok(Ok(Some(request))) => signer.answer(&request),
            Ok(Ok(None)) | Err(_) => return Ok(()),
            Ok(Err(error)) => return Err(error),
        };
        let mut framed = Vec::with_capacity(4 + reply.len());
        put_string(&mut framed, &reply);
        // A client that never reads is dropped too, not waited on.
        match tokio::time::timeout(REQUEST_TIMEOUT, stream.write_all(&framed)).await {
            Ok(written) => written?,
            Err(_) => return Ok(()),
        }
    }
}

/// One request; `None` at the stream's end or for one out of bounds.
async fn read_request(
    stream: &mut UnixStream,
) -> std::io::Result<Option<zeroize::Zeroizing<Vec<u8>>>> {
    let mut length = [0u8; 4];
    match stream.read_exact(&mut length).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_REQUEST {
        return Ok(None);
    }
    let mut request = zeroize::Zeroizing::new(vec![0u8; length]);
    stream.read_exact(&mut request).await?;
    Ok(Some(request))
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
