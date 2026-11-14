//! The SSH key an application signs in to its Agent37 instances with.
//!
//! Agent37 registers keys for a whole workspace (at most 20) and cannot
//! scope one to an instance, so an application keeps one key, registers it
//! once with [`super::Agent37Machines::register`], and stores the private
//! half itself. Revoking it is deleting it in Agent37's dashboard or API.
use super::failure::Agent37Failure;
use crate::providers::openssh::KeyPair;

/// An ed25519 key pair in OpenSSH's formats. `Debug` never prints the
/// private half.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent37Identity(KeyPair);
impl Agent37Identity {
    /// A key pair the application saved earlier.
    ///
    /// # Errors
    /// [`Agent37Failure::InvalidInput`] unless `private` is an OpenSSH
    /// private key file and `public` one `ssh-ed25519` public key.
    pub fn new(private: Vec<u8>, public: &str) -> Result<Self, Agent37Failure> {
        KeyPair::new(private, public)
            .map(Self)
            .map_err(|_| Agent37Failure::InvalidInput)
    }
    /// A new key pair, made by the system's `ssh-keygen`, labelled `label`.
    ///
    /// # Errors
    /// [`Agent37Failure::Provider`] when `ssh-keygen` fails or is missing.
    pub async fn generate(label: &str) -> Result<Self, Agent37Failure> {
        KeyPair::generate(label)
            .await
            .map(Self)
            .map_err(|_| Agent37Failure::Provider)
    }
    /// The private key file, to store.
    pub fn private_key(&self) -> &[u8] {
        self.0.private()
    }
    /// The public key, as `ssh-ed25519 <base64>`, to register and store.
    pub fn public_key(&self) -> &str {
        self.0.public()
    }
    pub(super) fn pair(&self) -> &KeyPair {
        &self.0
    }
}
