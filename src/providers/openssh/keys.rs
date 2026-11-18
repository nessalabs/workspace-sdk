//! Ed25519 keys in OpenSSH's formats: made in memory, and checked in full
//! before anything is trusted. A private key is never written to disk here.
use crate::{Error, Result};
use base64::Engine;
use ed25519_dalek::SigningKey;
use zeroize::Zeroizing;

/// The most an OpenSSH private key file takes here.
const MAX_PRIVATE: usize = 16 * 1024;
const KEY_TYPE: &[u8] = b"ssh-ed25519";
const ARMOUR_BEGIN: &str = "-----BEGIN OPENSSH PRIVATE KEY-----";
const ARMOUR_END: &str = "-----END OPENSSH PRIVATE KEY-----";

/// An ed25519 key pair. `Debug` never prints the private half.
#[derive(Clone, PartialEq, Eq)]
pub(in crate::providers) struct KeyPair {
    private: Zeroizing<Vec<u8>>,
    seed: Zeroizing<[u8; 32]>,
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
    /// [`Error::InvalidInput`] unless `private` is an unencrypted OpenSSH
    /// private key file holding exactly one ed25519 key, and `public` is
    /// that key's well-formed `ssh-ed25519` line.
    pub fn new(private: Vec<u8>, public: &str) -> Result<Self> {
        let private = Zeroizing::new(private);
        let invalid = || Error::InvalidInput("not an unencrypted ed25519 OpenSSH key pair".into());
        let public = ed25519_key(public.trim()).ok_or_else(invalid)?;
        let seed = (private.len() <= MAX_PRIVATE)
            .then(|| openssh_seed(&private))
            .flatten()
            .ok_or_else(invalid)?;
        if public_line(&SigningKey::from_bytes(&seed)) != public {
            return Err(invalid());
        }
        Ok(Self {
            private,
            seed,
            public,
        })
    }
    /// A new pair from the system's randomness, labelled `label`.
    ///
    /// # Errors
    /// [`Error::State`] when the system gives no randomness.
    pub async fn generate(label: &str) -> Result<Self> {
        let label: String = label
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || "-_.@".contains(*c))
            .take(64)
            .collect();
        let mut seed = Zeroizing::new([0u8; 32]);
        let mut check = [0u8; 4];
        getrandom::getrandom(seed.as_mut())
            .and_then(|()| getrandom::getrandom(&mut check))
            .map_err(|_| Error::State("no randomness for a new key".into()))?;
        let key = SigningKey::from_bytes(&seed);
        let public = public_line(&key);
        let private = openssh_file(&key, &label, check);
        Ok(Self {
            private,
            seed,
            public,
        })
    }
    /// The private key file.
    pub fn private(&self) -> &[u8] {
        &self.private
    }
    /// The public key, as `ssh-ed25519 <base64>`.
    pub fn public(&self) -> &str {
        &self.public
    }
    /// The key that signs for this pair.
    pub(super) fn signing_key(&self) -> SigningKey {
        SigningKey::from_bytes(&self.seed)
    }
}

/// `key`'s public half in SSH's wire form.
pub(super) fn public_blob(key: &SigningKey) -> Vec<u8> {
    let mut blob = Vec::with_capacity(51);
    put_string(&mut blob, KEY_TYPE);
    put_string(&mut blob, key.verifying_key().as_bytes());
    blob
}

fn public_line(key: &SigningKey) -> String {
    format!(
        "ssh-ed25519 {}",
        base64::engine::general_purpose::STANDARD.encode(public_blob(key))
    )
}

/// `key` as an unencrypted OpenSSH private key file, as `ssh-keygen` writes
/// one.
fn openssh_file(key: &SigningKey, comment: &str, check: [u8; 4]) -> Zeroizing<Vec<u8>> {
    let public = key.verifying_key();
    let mut pair = Zeroizing::new(Vec::with_capacity(64));
    pair.extend_from_slice(key.as_bytes());
    pair.extend_from_slice(public.as_bytes());
    let mut section = Zeroizing::new(Vec::with_capacity(160));
    section.extend_from_slice(&check);
    section.extend_from_slice(&check);
    put_string(&mut section, KEY_TYPE);
    put_string(&mut section, public.as_bytes());
    put_string(&mut section, &pair);
    put_string(&mut section, comment.as_bytes());
    // Padded to the cipher block size, 8 for "none", with 1, 2, 3, ...
    let mut pad = 1u8;
    while section.len() % 8 != 0 {
        section.push(pad);
        pad += 1;
    }
    let mut body = Zeroizing::new(Vec::with_capacity(320));
    body.extend_from_slice(b"openssh-key-v1\0");
    put_string(&mut body, b"none");
    put_string(&mut body, b"none");
    put_string(&mut body, b"");
    body.extend_from_slice(&1u32.to_be_bytes());
    put_string(&mut body, &public_blob(key));
    put_string(&mut body, &section);
    let encoded = Zeroizing::new(base64::engine::general_purpose::STANDARD.encode(&*body));
    let mut file = Zeroizing::new(Vec::with_capacity(encoded.len() + 100));
    file.extend_from_slice(ARMOUR_BEGIN.as_bytes());
    file.push(b'\n');
    for line in encoded.as_bytes().chunks(70) {
        file.extend_from_slice(line);
        file.push(b'\n');
    }
    file.extend_from_slice(ARMOUR_END.as_bytes());
    file.push(b'\n');
    file
}

/// The 32-byte seed of the one unencrypted ed25519 key in an OpenSSH
/// private key file.
fn openssh_seed(file: &[u8]) -> Option<Zeroizing<[u8; 32]>> {
    let text = std::str::from_utf8(file).ok()?.trim();
    let body = text.strip_prefix(ARMOUR_BEGIN)?.strip_suffix(ARMOUR_END)?;
    let body: Zeroizing<String> =
        Zeroizing::new(body.chars().filter(|c| !c.is_ascii_whitespace()).collect());
    let decoded = Zeroizing::new(
        base64::engine::general_purpose::STANDARD
            .decode(body.as_bytes())
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
    let mut seed = Zeroizing::new([0u8; 32]);
    seed.copy_from_slice(&pair[..32]);
    Some(seed)
}

/// Reads one SSH `uint32`.
pub(super) fn take_u32(input: &mut &[u8]) -> Option<u32> {
    let (head, rest) = input.split_first_chunk::<4>()?;
    *input = rest;
    Some(u32::from_be_bytes(*head))
}

/// Reads one SSH `string`.
pub(super) fn take_string<'a>(input: &mut &'a [u8]) -> Option<&'a [u8]> {
    let length = take_u32(input)? as usize;
    if length > input.len() {
        return None;
    }
    let (value, rest) = input.split_at(length);
    *input = rest;
    Some(value)
}

/// Writes one SSH `string`. Every value here is far below 4 GiB.
pub(super) fn put_string(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u32).to_be_bytes());
    output.extend_from_slice(value);
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

#[cfg(test)]
#[path = "keys_tests.rs"]
mod tests;
