//! Ed25519 keys in OpenSSH's formats: made by the system's `ssh-keygen`,
//! and checked before anything is trusted or written.
use crate::{Error, Result};
use base64::Engine;
use std::{process::Stdio, time::Duration};
use tokio::process::Command;

const KEYGEN_TIMEOUT: Duration = Duration::from_secs(10);
/// The most an OpenSSH private key file takes here.
const MAX_PRIVATE: usize = 16 * 1024;

/// An ed25519 key pair. `Debug` never prints the private half.
#[derive(Clone, PartialEq, Eq)]
pub(in crate::providers) struct KeyPair {
    private: zeroize::Zeroizing<Vec<u8>>,
    public: String,
}
impl std::fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyPair")
            .field("public", &self.public)
            .finish_non_exhaustive()
    }
}
impl KeyPair {
    /// A pair from its OpenSSH private key file and its public key line.
    ///
    /// # Errors
    /// [`Error::InvalidInput`] unless `private` is an OpenSSH private key
    /// file and `public` one well-formed `ssh-ed25519` key.
    pub fn new(private: Vec<u8>, public: &str) -> Result<Self> {
        let pem = std::str::from_utf8(&private).ok().map(str::trim);
        let private_ok = private.len() <= MAX_PRIVATE
            && pem.is_some_and(|pem| {
                pem.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----")
                    && pem.ends_with("-----END OPENSSH PRIVATE KEY-----")
            });
        let public = ed25519_key(public.trim())
            .filter(|_| private_ok)
            .ok_or_else(|| Error::InvalidInput("not an ed25519 OpenSSH key pair".into()))?;
        Ok(Self {
            private: zeroize::Zeroizing::new(private),
            public,
        })
    }
    /// A new pair, labelled `label`.
    pub async fn generate(label: &str) -> Result<Self> {
        let directory = tempfile::tempdir()?;
        let key = directory.path().join("key");
        let label: String = label
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || "-_.@".contains(*c))
            .take(64)
            .collect();
        let mut generate = Command::new("ssh-keygen");
        generate
            .args(["-q", "-t", "ed25519", "-N", "", "-C", &label, "-f"])
            .arg(&key)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let made = tokio::time::timeout(KEYGEN_TIMEOUT, generate.status())
            .await
            .map_err(|_| Error::Timeout("making a key timed out".into()))??;
        if !made.success() {
            return Err(Error::State("ssh-keygen failed".into()));
        }
        // Read into a buffer that is wiped when dropped; the file goes with
        // its directory.
        let private = tokio::fs::read(&key).await?;
        let public = tokio::fs::read_to_string(key.with_extension("pub")).await?;
        Self::new(private, &public)
    }
    /// The private key file.
    pub fn private(&self) -> &[u8] {
        &self.private
    }
    /// The public key, as `ssh-ed25519 <base64>`.
    pub fn public(&self) -> &str {
        &self.public
    }
}

/// `line`'s ed25519 key as `ssh-ed25519 <base64>`, if it is one `ssh-ed25519`
/// key whose bytes are an ed25519 key (a comment after it is dropped):
/// never an arbitrary line.
pub(in crate::providers) fn ed25519_key(line: &str) -> Option<String> {
    if line.contains(['\r', '\n']) {
        return None;
    }
    let mut parts = line.split_ascii_whitespace();
    let kind = parts.next()?;
    let encoded = parts.next()?;
    if kind != "ssh-ed25519" || encoded.len() > 256 {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    // The string "ssh-ed25519", then the 32 bytes of the key.
    let mut header = Vec::with_capacity(19);
    header.extend_from_slice(&11u32.to_be_bytes());
    header.extend_from_slice(b"ssh-ed25519");
    header.extend_from_slice(&32u32.to_be_bytes());
    if bytes.len() != 51 || bytes[..19] != header[..] {
        return None;
    }
    Some(format!("{kind} {encoded}"))
}
