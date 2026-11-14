//! Live connections into a provider's machine over the system's OpenSSH.
mod connection;
pub(in crate::providers) use connection::{Route, SshConnection, Way, HOST_ALIAS};
