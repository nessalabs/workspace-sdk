use super::*;
use crate::Result;
use async_trait::async_trait;

#[async_trait]
/// Persistence interface for nonsecret runtime preparation progress.
///
/// Receipts are keyed by preparation identity and must survive reconnects when
/// durable resumption is required. Loading a receipt never grants launch authority.
pub trait IPreparationStore: Send + Sync {
    /// Acquire exclusive operation ownership for this preparation, shared across
    /// store/service instances addressing the same persistence namespace. Fail
    /// promptly with Conflict while busy; do not wait behind long setup work.
    /// Hold the guard across load, side effects, and save. All controllers must
    /// use the same coordinated store for a preparation.
    async fn try_acquire(&self, id: &str) -> Result<Box<dyn IPreparationLease>>;
    async fn load(&self, id: &str) -> Result<Option<PreparationReceipt>>;
    async fn save(&self, receipt: &PreparationReceipt) -> Result<()>;
}

/// Exclusive preparation operation ownership, released when dropped. Store
/// adapters own its implementation; it carries no receipt or credential data.
pub trait IPreparationLease: Send + Sync {}
