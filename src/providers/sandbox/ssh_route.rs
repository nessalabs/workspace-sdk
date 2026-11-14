//! How a connection reaches a Boat VM: Boat authorizes a fresh key made for
//! the connection and names the VM's endpoint and host key, which is pinned.
//!
//! Boat cannot revoke a key: each connection leaves one more authorized key
//! on the machine, whose private half is deleted with the connection.
use crate::{
    providers::openssh::{Route, Way, HOST_ALIAS},
    Error, Result,
};
use base64::Engine;
use sandbox_client::Api;
use std::{net::SocketAddr, path::Path, process::Stdio, time::Duration};
use tokio::process::Command;

/// The only account Boat signs keys in as.
const USER: &str = "user";
const KEY_TIMEOUT: Duration = Duration::from_secs(10);

fn route_error(message: &str) -> Error {
    Error::Provider {
        provider: super::branding::NAME.into(),
        message: format!("connection: {message}"),
    }
}

/// Make a key in `directory`, have Boat authorize it on `vm`, and pin the
/// host key Boat names.
pub(super) async fn route(api: &Api, vm: &str, directory: &Path) -> Result<Route> {
    let public_key = make_key(directory).await?;
    let (endpoint, host_key) = authorize(api, vm, public_key).await?;
    tokio::fs::write(directory.join("hosts"), host_key).await?;
    Ok(Route {
        user: USER.into(),
        way: Way::Direct(endpoint),
        keep: None,
    })
}

async fn make_key(directory: &Path) -> Result<String> {
    let key = directory.join("key");
    let mut generate = Command::new("ssh-keygen");
    generate
        .args(["-q", "-t", "ed25519", "-N", "", "-C", HOST_ALIAS, "-f"])
        .arg(&key)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let made = tokio::time::timeout(KEY_TIMEOUT, generate.status())
        .await
        .map_err(|_| route_error("making a key timed out"))??;
    if !made.success() {
        return Err(route_error("ssh-keygen failed"));
    }
    Ok(tokio::fs::read_to_string(key.with_extension("pub")).await?)
}

/// Have Boat authorize `public_key` on `vm`; answer where to connect and the
/// `known_hosts` line to pin. Boat must name an IP endpoint and an ed25519
/// host key, or there is nothing to check the machine against.
async fn authorize(api: &Api, vm: &str, public_key: String) -> Result<(SocketAddr, String)> {
    let authorized = api
        .ssh_key(vm, public_key)
        .await
        .map_err(super::error::error)?;
    if !authorized.ok
        || authorized.success != Some(true)
        || authorized.ssh_user.as_deref() != Some(USER)
    {
        return Err(route_error("the key was not authorized"));
    }
    let endpoint = authorized
        .ssh_endpoint
        .as_deref()
        .and_then(|endpoint| endpoint.parse().ok())
        .ok_or_else(|| route_error("no usable SSH endpoint"))?;
    let host_key = known_host(authorized.host_key.as_deref())
        .ok_or_else(|| route_error("no usable host key"))?;
    Ok((endpoint, host_key))
}

/// The `known_hosts` line for `host_key`, which must be one `ssh-ed25519`
/// key whose bytes are an ed25519 key: never an arbitrary provider line.
fn known_host(host_key: Option<&str>) -> Option<String> {
    let host_key = host_key?;
    if host_key.contains(['\r', '\n']) {
        return None;
    }
    let mut parts = host_key.split_ascii_whitespace();
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
    Some(format!("{HOST_ALIAS} {kind} {encoded}\n"))
}

#[cfg(test)]
#[path = "ssh_route_tests.rs"]
mod tests;
