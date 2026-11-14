//! The SSH key an application signs in to its Agent37 instances with.
//!
//! Agent37 registers keys for a whole workspace (at most 20) and cannot
//! scope one to an instance, so an application keeps one key, registers it
//! once with [`super::Agent37Machines::register`], and stores the private
//! half itself. Revoking it is deleting it in Agent37's dashboard or API.
use super::failure::Agent37Failure;
use std::{os::unix::fs::OpenOptionsExt, path::Path, process::Stdio, time::Duration};

const KEYGEN_TIMEOUT: Duration = Duration::from_secs(10);
/// The most an OpenSSH private key file takes here.
const MAX_PRIVATE: usize = 16 * 1024;

/// An ed25519 key pair in OpenSSH's formats. `Debug` never prints the
/// private half.
#[derive(Clone, PartialEq, Eq)]
pub struct Agent37Identity {
    private: Vec<u8>,
    public: String,
}
impl std::fmt::Debug for Agent37Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agent37Identity")
            .field("public", &self.public)
            .finish_non_exhaustive()
    }
}
impl Agent37Identity {
    /// A key pair the application saved earlier.
    ///
    /// # Errors
    /// [`Agent37Failure::InvalidInput`] unless `private` is an OpenSSH
    /// private key file and `public` one `ssh-ed25519` public key line.
    pub fn new(private: Vec<u8>, public: impl Into<String>) -> Result<Self, Agent37Failure> {
        let public = public.into().trim().to_owned();
        let pem = std::str::from_utf8(&private).ok();
        let private_ok = private.len() <= MAX_PRIVATE
            && pem.is_some_and(|pem| {
                pem.trim_start()
                    .starts_with("-----BEGIN OPENSSH PRIVATE KEY-----")
                    && pem
                        .trim_end()
                        .ends_with("-----END OPENSSH PRIVATE KEY-----")
            });
        let mut parts = public.split_ascii_whitespace();
        let public_ok = parts.next() == Some("ssh-ed25519")
            && parts.next().is_some_and(|key| key.len() <= 256)
            && !public.contains(['\r', '\n']);
        if !private_ok || !public_ok {
            return Err(Agent37Failure::InvalidInput);
        }
        Ok(Self { private, public })
    }
    /// A new key pair, made by the system's `ssh-keygen`, labelled `label`.
    ///
    /// # Errors
    /// [`Agent37Failure::Provider`] when `ssh-keygen` fails or is missing.
    pub async fn generate(label: &str) -> Result<Self, Agent37Failure> {
        let directory = tempfile::tempdir().map_err(|_| Agent37Failure::Provider)?;
        let key = directory.path().join("key");
        let label: String = label
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || "-_.@".contains(*c))
            .take(64)
            .collect();
        let mut generate = tokio::process::Command::new("ssh-keygen");
        generate
            .args(["-q", "-t", "ed25519", "-N", "", "-C", &label, "-f"])
            .arg(&key)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let made = tokio::time::timeout(KEYGEN_TIMEOUT, generate.status())
            .await
            .map_err(|_| Agent37Failure::Provider)?
            .map_err(|_| Agent37Failure::Provider)?;
        if !made.success() {
            return Err(Agent37Failure::Provider);
        }
        let private = std::fs::read(&key).map_err(|_| Agent37Failure::Provider)?;
        let public = std::fs::read_to_string(key.with_extension("pub"))
            .map_err(|_| Agent37Failure::Provider)?;
        Self::new(private, public)
    }
    /// The private key file, to store.
    pub fn private_key(&self) -> &[u8] {
        &self.private
    }
    /// The public key line, to register and store.
    pub fn public_key(&self) -> &str {
        &self.public
    }
    /// Write the private key to `path`, readable by this account alone.
    pub(super) fn write_private(&self, path: &Path) -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        std::io::Write::write_all(&mut file, &self.private)
    }
}
