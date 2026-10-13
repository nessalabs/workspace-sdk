use super::*;
use crate::Result;
use async_trait::async_trait;

#[async_trait]
/// Application-supplied interface for resolving credential references at the point of use.
///
/// Implementations own secret storage and expiry knowledge. They return transient
/// material or a typed unavailable state without adding that material to receipts.
pub trait ICredentialResolver: Send + Sync {
    async fn resolve(&self, credential: &CredentialRef) -> Result<CredentialResolution>;
}
