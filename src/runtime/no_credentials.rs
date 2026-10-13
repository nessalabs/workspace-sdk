use super::*;
use crate::Result;
use async_trait::async_trait;

/// Useful for workloads without secrets; never reads ambient environment values.
pub struct NoCredentials;

#[async_trait]
impl ICredentialResolver for NoCredentials {
    async fn resolve(&self, _credential: &CredentialRef) -> Result<CredentialResolution> {
        Ok(CredentialResolution::Missing)
    }
}
