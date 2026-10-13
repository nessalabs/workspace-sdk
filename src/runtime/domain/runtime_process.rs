//! References to external native-login processes and their observed state.
//!
//! The host owns each process entity. The reference is a persistable identity value,
//! and the state is an observation used to reconnect or decide whether retry is safe.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// The host's current observation of a tracked native authentication process.
///
/// This snapshot guides reconnection and retry. It is distinct from authentication
/// success, which is determined by the native CLI's status check.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeProcessState {
    /// The existing login process is still running; reconnect rather than start a duplicate.
    Running,
    /// The tracked process exited with the reported code. A later attempt may use
    /// a new idempotency key; even exit zero still requires a native auth status check.
    Exited {
        /// Host-reported exit code; zero alone is not proof of authentication success.
        code: i32,
    },
    /// The host cannot establish the process's state. Preparation refuses to start
    /// another login attempt until the uncertainty is reconciled.
    Lost,
}

/// An opaque, serializable reference to one host-owned authentication process.
///
/// The reference is a value identifying a process entity managed by
/// [`crate::runtime::IRuntimeHost`]; it does not own or cancel that process when dropped. It is
/// stored in the preparation receipt so another service instance can reconnect.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ProcessIdentity")]
pub struct RuntimeProcessRef {
    host_scope: String,
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessIdentity {
    host_scope: String,
    id: String,
}

impl TryFrom<ProcessIdentity> for RuntimeProcessRef {
    type Error = Error;

    fn try_from(value: ProcessIdentity) -> Result<Self> {
        Self::new(value.host_scope, value.id)
    }
}

impl RuntimeProcessRef {
    /// Bind a process identity to the exact host instance namespace. Host scopes
    /// must include enough target identity to distinguish equal IDs on different
    /// providers or storage roots. Neither value may contain control characters
    /// or exceed 256 bytes. Legacy unscoped serialized references are rejected.
    pub fn new(host_scope: impl Into<String>, id: impl Into<String>) -> Result<Self> {
        let host_scope = host_scope.into();
        let id = id.into();
        if [&host_scope, &id].iter().any(|value| {
            value.is_empty() || value.len() > 256 || value.chars().any(char::is_control)
        }) {
            return Err(Error::InvalidInput(
                "invalid native process identity".into(),
            ));
        }
        Ok(Self { host_scope, id })
    }

    /// Exact adapter-owned target scope; never substitute the current host's
    /// scope for a saved reference's scope.
    pub fn host_scope(&self) -> &str {
        &self.host_scope
    }

    /// Host-specific process identity. Treat it as opaque and inspect it through the
    /// same target host rather than interpreting it as an operating-system PID.
    pub fn id(&self) -> &str {
        &self.id
    }
}
