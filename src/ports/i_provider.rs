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
    /// Every attempt for one workspace has the same [`CreateRequest::provision_key`];
    /// a provider that provisions must use it as its idempotency key, so a retry
    /// after an unknown outcome finds the first target instead of making a second. Return
    /// [`crate::Error::Refused`] for a target that must never be used; any other
    /// error leaves the attempt retryable. The returned descriptor must report
    /// [`Ownership::SdkOwned`]. Providers that cannot create refuse with
    /// `Unsupported`.
    async fn create(&self, request: CreateRequest) -> Result<Arc<dyn IBackend>>;
    /// Check a create request without any effect: decode its options and
    /// refuse what [`Self::create`] would refuse before asking for anything.
    /// The SDK calls it before reserving the ID, so a request that can never
    /// succeed does not leave an unfinished record behind.
    fn check_create(&self, request: &CreateRequest) -> Result<()>;
    /// Check an attach request without any effect, as [`Self::check_create`].
    fn check_attach(&self, request: &AttachRequest) -> Result<()>;
    /// Use an existing target without provisioning it or changing its project files.
    ///
    /// The returned descriptor must report [`Ownership::Attached`]. Providers that
    /// cannot attach refuse with `Unsupported`.
    async fn attach(&self, request: AttachRequest) -> Result<Arc<dyn IBackend>>;
    /// Resolve a saved locator without silently recreating missing execution state.
    async fn connect(&self, reference: &WorkspaceRef) -> Result<Arc<dyn IBackend>>;
}
