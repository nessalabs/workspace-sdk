//! Service access requests, policies, and returned endpoints.
use serde::{Deserialize, Serialize};
use std::fmt;

/// A request to access a service already listening inside the workspace.
///
/// This exposes access to a port; it does not start the service.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortRequest {
    /// TCP port of the service on the target.
    pub port: u16,
    /// Required access policy for the returned endpoint.
    pub policy: PortPolicy,
}

/// Requested access boundary for an exposed workspace service.
///
/// A provider may reject policies it cannot implement; there is no implicit
/// fallback to a publicly accessible unauthenticated endpoint.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortPolicy {
    /// Access through a private or local endpoint without public hosting.
    Private,
    /// Access through a hosted endpoint protected by provider authentication.
    Authenticated,
}

/// A usable endpoint returned after a backend exposes a workspace port.
///
/// The URL can contain a bearer token. `Debug` redacts it, but serialization
/// includes it so a client can open the endpoint; handle that serialized value as sensitive.
#[derive(Clone, Serialize, Deserialize)]
pub struct PortAccess {
    /// Endpoint URL, potentially including a temporary access credential.
    pub url: String,
    /// Access policy actually used for this endpoint.
    pub policy: PortPolicy,
}
impl fmt::Debug for PortAccess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PortAccess")
            .field("url", &"<redacted>")
            .field("policy", &self.policy)
            .finish()
    }
}

#[cfg(test)]
mod port_access_tests {
    use super::*;
    #[test]
    fn debug_redacts_signed_url() {
        let access = PortAccess {
            url: "secret-url".into(),
            policy: PortPolicy::Authenticated,
        };
        assert!(!format!("{access:?}").contains("secret-url"));
    }
}
