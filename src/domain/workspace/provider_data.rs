//! Explicit extension points for provider-specific configuration and locators.
use crate::Result;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// Serializable provider options constructed from a provider’s named options type.
///
/// Each provider owns its configuration schema (for example `SandboxOpenOptions`).
/// This value erases that concrete type only at the registry boundary so custom
/// providers can be injected without changing the core domain. Decode it back
/// into the provider’s options immediately inside the adapter. Never put secrets
/// here; credentials belong in the injected provider client.
///
/// This is a configuration value object. `Debug` hides its payload, while
/// serialization retains it; the wrapper does not detect or remove credentials.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProviderOptions(Value);

impl ProviderOptions {
    /// Encode a concrete provider options struct for registry dispatch.
    pub fn from_options<T: Serialize>(options: &T) -> Result<Self> {
        Ok(Self(serde_json::to_value(options)?))
    }

    /// Deserialize using the selected provider's concrete options schema.
    /// The adapter remains responsible for enforcing semantic requirements.
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T> {
        Ok(serde_json::from_value(self.0.clone())?)
    }

    /// Whether the encoded value is null, as produced by `Default`.
    /// An explicitly supplied empty options object is not considered absent.
    pub fn is_empty(&self) -> bool {
        self.0.is_null()
    }
}

impl fmt::Debug for ProviderOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ProviderOptions(<provider configuration>)")
    }
}

/// Opaque, non-secret reconnect metadata owned by one provider’s schema.
///
/// A locator is a value object, not a live client or mutable target entity. The
/// creating provider encodes a named locator struct and decodes it on reconnect.
/// Other contexts carry it without interpreting provider-specific fields.
/// `Debug` hides the payload, while serialization retains the reconnect metadata.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkspaceLocator(Value);

impl WorkspaceLocator {
    /// Encode the concrete provider locator to persist in a workspace reference.
    pub fn from_locator<T: Serialize>(locator: &T) -> Result<Self> {
        Ok(Self(serde_json::to_value(locator)?))
    }

    /// Decode reconnect metadata using the owning provider’s concrete schema.
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T> {
        Ok(serde_json::from_value(self.0.clone())?)
    }
}

impl fmt::Debug for WorkspaceLocator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WorkspaceLocator(<provider reconnect metadata>)")
    }
}
