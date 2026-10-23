use super::file_tree::*;
use super::filesystem::*;
use super::git_checkpoint_engine::GitCheckpointEngine;
use super::scanner::*;
use crate::checkpoint::domain::*;
use crate::{Error, Result};
use std::fs;
use std::path::Path;
use uuid::Uuid;

impl GitCheckpointEngine {
    pub(super) fn replace_selected(
        &self,
        destination: &Path,
        before: &Tree,
        after: &Tree,
    ) -> Result<()> {
        // Remove only declared files, preserving all excluded and unrelated data.
        remove_selected(destination, &before.entries(), false)?;
        write_tree(destination, after)
    }

    pub(super) fn rollback_inner(&self, journal: &mut RecoveryJournal) -> Result<()> {
        let before = self.load_tree(&journal.before)?;
        let after = self.load_tree(&journal.after)?;
        let mut removal = after.entries();
        for entry in before.entries() {
            if !removal.iter().any(|e| e.path == entry.path) {
                removal.push(entry);
            }
        }
        // Remove deeper paths first to support file/directory transitions.
        removal.sort_by(|a, b| {
            b.path
                .matches('/')
                .count()
                .cmp(&a.path.matches('/').count())
                .then_with(|| b.path.cmp(&a.path))
        });
        remove_selected(&journal.destination, &removal, true)?;
        write_tree(&journal.destination, &before)?;
        journal.status = "rolled_back".into();
        self.write_json(
            &self.recovery_dir(&journal.reference)?.join("journal.json"),
            journal,
        )
    }

    pub(super) fn applied_receipt(
        &self,
        reconciliation: &mut Reconciliation,
        journal: &mut RecoveryJournal,
        fingerprint: String,
    ) -> Result<ApplyReceipt> {
        let receipt = ApplyReceipt {
            reconciliation: reconciliation.reference.clone(),
            checkpoint: journal.after.clone(),
            recovery: journal.reference.clone(),
            fingerprint,
        };
        journal.status = "applied".into();
        self.write_json(
            &self.recovery_dir(&journal.reference)?.join("journal.json"),
            journal,
        )?;
        reconciliation.status = ReconciliationStatus::Applied;
        reconciliation.applied = Some(receipt.clone());
        self.save_reconciliation(reconciliation)?;
        Ok(receipt)
    }

    pub(super) fn apply_operation(
        &self,
        reference: &ReconciliationRef,
        options: ApplyOptions,
    ) -> Result<ApplyReceipt> {
        let _lock = self.lock()?;
        if !options.quiescent {
            return Err(Error::InvalidInput(
                "apply requires caller-established exclusive access (quiescent=true)".into(),
            ));
        }
        let mut reconciliation = self.load_reconciliation(reference)?;
        if options.expected_fingerprint != reconciliation.expected_fingerprint {
            return Err(Error::Conflict(
                "apply fingerprint does not match the staged plan".into(),
            ));
        }
        if !matches!(
            reconciliation.status,
            ReconciliationStatus::StagedForReview | ReconciliationStatus::Applied
        ) {
            return Err(Error::Conflict(
                "reconciliation is not ready for apply".into(),
            ));
        }
        let result = reconciliation
            .result
            .clone()
            .ok_or_else(|| Error::Conflict("unresolved merge conflicts".into()))?;
        let before = self.load_tree(&reconciliation.local)?;
        let after = self.load_tree(&result)?;
        let mut existing_journal = if let Some(recovery) = &reconciliation.recovery {
            let journal: RecoveryJournal =
                self.read_json(&self.recovery_dir(recovery)?.join("journal.json"))?;
            if journal.reference != *recovery
                || journal.before != reconciliation.local
                || journal.after != result
                || journal.destination != reconciliation.destination
            {
                return Err(Error::State(
                    "apply recovery does not match its reconciliation".into(),
                ));
            }
            Some(journal)
        } else {
            None
        };
        if let Some(journal) = &mut existing_journal {
            if journal.status == "applied" {
                // Returning the prior result is safe even if users edited later:
                // retries never reapply or claim current files remain unchanged.
                return self.applied_receipt(&mut reconciliation, journal, after.fingerprint()?);
            }
            if journal.status != "applying" {
                return Err(Error::Conflict(
                    "previous apply was rolled back; stage a new reconciliation".into(),
                ));
            }
            let selected = after.files.keys().cloned().collect();
            if scan(&reconciliation.destination, &selected, &self.limits)?.fingerprint()?
                == after.fingerprint()?
            {
                // The process may have stopped after writing the tree but before
                // advancing either durable receipt. Recover that exact result.
                return self.applied_receipt(&mut reconciliation, journal, after.fingerprint()?);
            }
        } else if reconciliation.status == ReconciliationStatus::Applied {
            return Err(Error::State(
                "applied reconciliation is missing its recovery receipt".into(),
            ));
        }
        if self.scan(&reconciliation.destination)?.fingerprint()?
            != reconciliation.expected_fingerprint
        {
            let message = existing_journal.as_ref().map(|journal| format!("apply interrupted or destination changed; inspect and rollback recovery {}", journal.reference.0))
                .unwrap_or_else(|| "destination changed after reconciliation; stage a new merge".into());
            return Err(Error::Conflict(message));
        }
        if self.review_tree(&reconciliation)?.fingerprint()? != after.fingerprint()? {
            return Err(Error::Conflict(
                "review directory changed; accept its resolved contents before apply".into(),
            ));
        }
        // Before mutation or recovery intent, reject every result-path collision
        // with data absent from the captured before tree. Otherwise automatic
        // rollback could remove a preexisting ignored file it never backed up.
        super::apply_preflight::preflight(
            &reconciliation.destination,
            &before,
            &after,
            &self.limits,
        )?;
        let mut journal = match existing_journal {
            Some(journal) => journal,
            None => {
                let recovery = RecoveryRef(Uuid::new_v4().to_string());
                let directory = self.recovery_dir(&recovery)?;
                fs::create_dir(&directory)?;
                let journal = RecoveryJournal {
                    version: 1,
                    reference: recovery.clone(),
                    destination: reconciliation.destination.clone(),
                    before: reconciliation.local.clone(),
                    after: result,
                    status: "applying".into(),
                };
                self.write_json(&directory.join("journal.json"), &journal)?;
                // Record the operation identity before changing any project file.
                reconciliation.recovery = Some(recovery);
                self.save_reconciliation(&reconciliation)?;
                journal
            }
        };
        if let Err(error) = self.replace_selected(&reconciliation.destination, &before, &after) {
            let rollback = self.rollback_inner(&mut journal);
            return Err(Error::State(format!(
                "apply failed: {error}; recovery {}: {}",
                journal.reference.0,
                match rollback {
                    Ok(()) => "rolled back".to_owned(),
                    Err(error) => format!("rollback requires attention: {error}"),
                }
            )));
        }
        let selected = after.files.keys().cloned().collect();
        if scan(&reconciliation.destination, &selected, &self.limits)?.fingerprint()?
            != after.fingerprint()?
        {
            return Err(Error::State(format!(
                "post-apply fingerprint mismatch; retain recovery {} and inspect writers",
                journal.reference.0
            )));
        }
        self.applied_receipt(&mut reconciliation, &mut journal, after.fingerprint()?)
    }

    pub(super) fn rollback_operation(
        &self,
        reference: &RecoveryRef,
        quiescent: bool,
    ) -> Result<()> {
        let _lock = self.lock()?;
        if !quiescent {
            return Err(Error::InvalidInput(
                "rollback requires exclusive destination access".into(),
            ));
        }
        let mut journal: RecoveryJournal =
            self.read_json(&self.recovery_dir(reference)?.join("journal.json"))?;
        if &journal.reference != reference || journal.version != 1 {
            return Err(Error::State("invalid recovery journal".into()));
        }
        self.rollback_inner(&mut journal)
    }
}
