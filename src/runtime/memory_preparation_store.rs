use super::{IPreparationLease, IPreparationStore, PreparationReceipt};
use crate::{Error, Result};
use async_trait::async_trait;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

#[derive(Default)]
pub struct MemoryPreparationStore {
    receipts: Mutex<BTreeMap<String, PreparationReceipt>>,
    active: Arc<Mutex<BTreeSet<String>>>,
}

struct MemoryLease {
    active: Arc<Mutex<BTreeSet<String>>>,
    id: String,
}
impl IPreparationLease for MemoryLease {}
impl Drop for MemoryLease {
    fn drop(&mut self) {
        self.active
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&self.id);
    }
}

#[async_trait]
impl IPreparationStore for MemoryPreparationStore {
    async fn try_acquire(&self, id: &str) -> Result<Box<dyn IPreparationLease>> {
        if !self
            .active
            .lock()
            .map_err(|_| Error::State("preparation lease lock poisoned".into()))?
            .insert(id.to_owned())
        {
            return Err(Error::Conflict(
                "preparation is busy; retry this operation later".into(),
            ));
        }
        Ok(Box::new(MemoryLease {
            active: self.active.clone(),
            id: id.to_owned(),
        }))
    }
    async fn load(&self, id: &str) -> Result<Option<PreparationReceipt>> {
        Ok(self
            .receipts
            .lock()
            .map_err(|_| Error::State("preparation store lock poisoned".into()))?
            .get(id)
            .cloned())
    }
    async fn save(&self, receipt: &PreparationReceipt) -> Result<()> {
        let mut receipts = self
            .receipts
            .lock()
            .map_err(|_| Error::State("preparation store lock poisoned".into()))?;
        if receipts.get(&receipt.id).is_some_and(|old| {
            old.request_fingerprint != receipt.request_fingerprint
                || old.host_scope != receipt.host_scope
                || old.target_id != receipt.target_id
        }) {
            return Err(Error::Conflict(
                "preparation id was used for a different request".into(),
            ));
        }
        receipts.insert(receipt.id.clone(), receipt.clone());
        Ok(())
    }
}
