//! Live connections into a provider's machine over the system's OpenSSH.
mod agent;
mod connection;
mod directories;
mod keys;
pub(in crate::providers) use connection::{Dial, Route, SshConnection, HOST_ALIAS};
pub(in crate::providers) use keys::{ed25519_key, KeyPair};
