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
//! Only the connection's own ssh processes can reach the socket: it lives
//! in the connection's directory, which only this account may enter. The
//! agent answers just the two requests ssh needs to sign in, for its one
//! key, and fails everything else. A process killed outright leaves the
//! socket file behind, never the key.
use crate::{Error, Result};
use base64::Engine;
use ed25519_dalek::{Signer as _, SigningKey};
use std::{path::Path, sync::Arc};
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
/// The most requests served at once, so a stuck client cannot pile up tasks.
const MAX_CLIENTS: usize = 32;
const KEY_TYPE: &[u8] = b"ssh-ed25519";

/// An ed25519 key that signs in memory.
pub(super) struct Signer {
    key: SigningKey,
    /// The public key in SSH's wire form.
    blob: Vec<u8>,
}

impl Signer {
    /// The signer for an unencrypted OpenSSH ed25519 private key file whose
    /// public half is `public` (`ssh-ed25519 <base64>`).
    ///
    /// # Errors
    /// [`Error::InvalidInput`] unless `private` holds exactly one
    /// unencrypted ed25519 key, and that key is `public`.
    pub(super) fn new(private: &[u8], public: &str) -> Result<Self> {
        let invalid = || Error::InvalidInput("not an unencrypted ed25519 OpenSSH key".into());
        let blob = public
            .split_ascii_whitespace()
            .nth(1)
            .and_then(|encoded| {
                base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .ok()
            })
            .ok_or_else(invalid)?;
        let seed = openssh_seed(private).ok_or_else(invalid)?;
        let key = SigningKey::from_bytes(&seed);
        let mut expected = Vec::with_capacity(51);
        put_string(&mut expected, KEY_TYPE);
        put_string(&mut expected, key.verifying_key().as_bytes());
        if expected != blob {
            return Err(invalid());
        }
        Ok(Self { key, blob })
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
    /// Serve `signer` on a new socket at `socket`.
    pub(super) fn serve(signer: Signer, socket: &Path) -> Result<Self> {
        let listener = UnixListener::bind(socket)?;
        let signer = Arc::new(signer);
        let permits = Arc::new(tokio::sync::Semaphore::new(MAX_CLIENTS));
        let listener = tokio::spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else {
                            // Out of descriptors, say: wait rather than spin.
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            continue;
                        };
                        let Ok(permit) = permits.clone().try_acquire_owned() else {
                            // Too many at once: this one is closed unanswered.
                            continue;
                        };
                        let signer = signer.clone();
                        clients.spawn(async move {
                            let _ = serve_client(stream, &signer).await;
                            drop(permit);
                        });
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

async fn serve_client(mut stream: UnixStream, signer: &Signer) -> std::io::Result<()> {
    loop {
        let mut length = [0u8; 4];
        match stream.read_exact(&mut length).await {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error),
        }
        let length = u32::from_be_bytes(length) as usize;
        if length == 0 || length > MAX_REQUEST {
            return Ok(());
        }
        let mut request = zeroize::Zeroizing::new(vec![0u8; length]);
        stream.read_exact(&mut request).await?;
        let reply = signer.answer(&request);
        let mut framed = Vec::with_capacity(4 + reply.len());
        put_string(&mut framed, &reply);
        stream.write_all(&framed).await?;
    }
}

/// The 32-byte seed of the one unencrypted ed25519 key in an OpenSSH
/// private key file.
fn openssh_seed(file: &[u8]) -> Option<zeroize::Zeroizing<[u8; 32]>> {
    let text = std::str::from_utf8(file).ok()?.trim();
    let body = text
        .strip_prefix("-----BEGIN OPENSSH PRIVATE KEY-----")?
        .strip_suffix("-----END OPENSSH PRIVATE KEY-----")?;
    let body: String = body.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    let decoded = zeroize::Zeroizing::new(
        base64::engine::general_purpose::STANDARD
            .decode(body)
            .ok()?,
    );
    let mut rest = decoded.strip_prefix(b"openssh-key-v1\0")?;
    let cipher = take_string(&mut rest)?;
    let kdf = take_string(&mut rest)?;
    let _kdf_options = take_string(&mut rest)?;
    if cipher != b"none" || kdf != b"none" || take_u32(&mut rest)? != 1 {
        return None;
    }
    let _public = take_string(&mut rest)?;
    let mut private = take_string(&mut rest)?;
    let (check1, check2) = (take_u32(&mut private)?, take_u32(&mut private)?);
    if check1 != check2 || take_string(&mut private)? != KEY_TYPE {
        return None;
    }
    let public = take_string(&mut private)?;
    let pair = take_string(&mut private)?;
    // The private half is the seed followed by the public key.
    if public.len() != 32 || pair.len() != 64 || pair[32..] != *public {
        return None;
    }
    let mut seed = zeroize::Zeroizing::new([0u8; 32]);
    seed.copy_from_slice(&pair[..32]);
    Some(seed)
}

fn take_u32(input: &mut &[u8]) -> Option<u32> {
    let (head, rest) = input.split_first_chunk::<4>()?;
    *input = rest;
    Some(u32::from_be_bytes(*head))
}

fn take_string<'a>(input: &mut &'a [u8]) -> Option<&'a [u8]> {
    let length = take_u32(input)? as usize;
    if length > input.len() {
        return None;
    }
    let (value, rest) = input.split_at(length);
    *input = rest;
    Some(value)
}

fn put_string(output: &mut Vec<u8>, value: &[u8]) {
    // Every value here is far below 4 GiB.
    output.extend_from_slice(&(value.len() as u32).to_be_bytes());
    output.extend_from_slice(value);
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
