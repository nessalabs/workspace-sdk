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
    /// Create or attach using concrete provider options and the supplied stable ID.
    async fn open(&self, request: OpenRequest) -> Result<Arc<dyn IBackend>>;
    /// Resolve a saved locator without silently recreating missing execution state.
    async fn connect(&self, reference: &WorkspaceRef) -> Result<Arc<dyn IBackend>>;
}
