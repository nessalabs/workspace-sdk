use super::IBackend;
use crate::{domain::*, Result};
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
/// Interface for creating and reconnecting workspaces owned by one adapter registration.
///
/// The adapter owns its options/locator schemas and receives credentials through
/// its constructor. It returns a connected [`IBackend`]; it does not own the
/// application’s registry or persistence implementation.
pub trait IProvider: Send + Sync {
    /// Stable identity used to select this provider in the SDK registry.
    fn id(&self) -> ProviderId;
    /// Create and initialise a new SDK-owned target under the supplied stable ID.
    ///
    /// The returned descriptor must report [`Ownership::SdkOwned`]. Providers that
    /// cannot create targets refuse with `Unsupported`.
    async fn create(&self, request: CreateRequest) -> Result<Arc<dyn IBackend>>;
    /// Use an existing target without provisioning it or changing its project files.
    ///
    /// The returned descriptor must report [`Ownership::Attached`]. Providers that
    /// cannot attach refuse with `Unsupported`.
    async fn attach(&self, request: AttachRequest) -> Result<Arc<dyn IBackend>>;
    /// Resolve a saved locator without silently recreating missing execution state.
    async fn connect(&self, reference: &WorkspaceRef) -> Result<Arc<dyn IBackend>>;
}
