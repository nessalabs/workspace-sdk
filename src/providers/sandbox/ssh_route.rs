//! How a connection reaches a Boat VM: Boat authorizes a fresh key made for
//! the connection and names the VM's endpoint and host key, which is pinned.
//!
//! Boat cannot revoke a key: each connection leaves one more authorized key
//! on the machine, whose private half is deleted with the connection.
use crate::{
    providers::openssh::{ed25519_key, Dial, KeyPair, Route, HOST_ALIAS},
    Error, Result,
};
use sandbox_client::Api;
use std::net::SocketAddr;

/// The only account Boat signs keys in as.
const USER: &str = "user";

fn route_error(message: &str) -> Error {
    Error::Provider {
        provider: super::branding::NAME.into(),
        message: format!("connection: {message}"),
    }
}

/// Make a key, have Boat authorize it on `vm`, and pin the host key Boat
/// names.
pub(super) async fn route(api: &Api, vm: &str) -> Result<Route> {
    let key = KeyPair::generate(HOST_ALIAS).await?;
    let (endpoint, host_key) = authorize(api, vm, key.public().to_owned()).await?;
    Ok(Route {
        user: USER.into(),
        key,
        dial: Dial::Direct { endpoint, host_key },
        guard: None,
    })
}

/// Have Boat authorize `public_key` on `vm`; answer where to connect and the
/// host key to pin. Boat must name an IP endpoint and an ed25519 host key,
/// or there is nothing to check the machine against.
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
    let host_key = authorized
        .host_key
        .as_deref()
        .and_then(ed25519_key)
        .ok_or_else(|| route_error("no usable host key"))?;
    Ok((endpoint, host_key))
}

#[cfg(test)]
#[path = "ssh_route_tests.rs"]
mod tests;
