use super::git_checkpoint_engine::GitCheckpointEngine;
use crate::checkpoint::domain::*;
use crate::{Error, Result};

impl GitCheckpointEngine {
    pub(super) fn inspect_apply_operation(
        &self,
        reference: &ReconciliationRef,
    ) -> Result<ApplyState> {
        let _lock = self.lock()?;
        let reconciliation = self.load_reconciliation(reference)?;
        let Some(recovery) = reconciliation.recovery else {
            if reconciliation.status == ReconciliationStatus::Applied
                || reconciliation.applied.is_some()
            {
                return Err(Error::State(
                    "applied reconciliation is missing its recovery journal".into(),
                ));
            }
            return Ok(ApplyState::NotStarted);
        };
        let journal: RecoveryJournal =
            self.read_json(&self.recovery_dir(&recovery)?.join("journal.json"))?;
        if journal.version != 1
            || journal.reference != recovery
            || journal.before != reconciliation.local
            || Some(&journal.after) != reconciliation.result.as_ref()
            || journal.destination != reconciliation.destination
        {
            return Err(Error::State(
                "apply journal does not match its reconciliation".into(),
            ));
        }
        match journal.status.as_str() {
            "applying" => Ok(ApplyState::Interrupted { recovery }),
            "rolled_back" => Ok(ApplyState::RolledBack { recovery }),
            "applied" => {
                let checkpoint = self.load_checkpoint(&journal.after)?;
                let receipt = ApplyReceipt {
                    reconciliation: reference.clone(),
                    checkpoint: journal.after,
                    recovery,
                    fingerprint: checkpoint.manifest.tree_fingerprint,
                };
                Ok(ApplyState::Applied { receipt })
            }
            _ => Err(Error::State("unknown apply journal status".into())),
        }
    }
}
