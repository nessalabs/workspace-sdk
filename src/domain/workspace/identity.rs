//! Workspace and provider identity value objects. Values identify a resource; they do not own its mutable state.
use super::WorkspaceLocator;
use crate::domain::identifier::identifier;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

identifier!(
    /// Caller-selected workspace identity, independent of a provider’s resource ID.
    ///
    /// Keep it stable across retries. [`WorkspaceRef`] adds the provider locator
    /// needed to reconnect to the actual resource.
    WorkspaceId
);

#[cfg(test)]
mod workspace_id_tests {
    use super::*;
    #[test]
    fn validates_direct_and_deserialized_ids() {
        for value in ["", ".", "..", "../unsafe", "space name"] {
            assert!(WorkspaceId::new(value).is_err());
            assert!(serde_json::from_value::<WorkspaceId>(serde_json::json!(value)).is_err());
        }
        assert_eq!(
            WorkspaceId::new("valid-id_1").unwrap().as_str(),
            "valid-id_1"
        );
    }
}

identifier!(
    /// Identity of a provider registered in the SDK’s dependency-injection registry.
    ///
    /// A workspace reference uses this value to select the adapter that understands
    /// its locator. Distinct configurations can be registered under distinct IDs.
    ProviderId
);

#[cfg(test)]
mod provider_id_tests {
    use super::*;
    #[test]
    fn validates_direct_and_deserialized_ids() {
        for value in ["", ".", "..", "../unsafe", "space name"] {
            assert!(ProviderId::new(value).is_err());
            assert!(serde_json::from_value::<ProviderId>(serde_json::json!(value)).is_err());
        }
        assert_eq!(
            ProviderId::new("valid-id_1").unwrap().as_str(),
            "valid-id_1"
        );
    }
}

/// Serializable identity and provider locator used to reconnect to a workspace.
///
/// Persist this handle instead of a live backend connection. The provider
/// interprets `locator`; callers should treat it as opaque and supply credentials
/// separately through the injected provider client or its connection settings.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceRef {
    /// Handle format version; current built-in providers use version 1.
    pub version: u32,
    /// Stable workspace identity chosen by the SDK caller.
    pub id: WorkspaceId,
    /// Registered provider responsible for resolving the locator.
    pub provider: ProviderId,
    /// Opaque provider connection metadata. Must not contain credentials.
    pub locator: WorkspaceLocator,
}
