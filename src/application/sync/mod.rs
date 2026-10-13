//! Application services for the synchronization domain.
mod sync_service;

pub use crate::domain::synchronization::{HandoffPhase, HandoffReceipt, SyncSession};
pub use sync_service::SyncService;
