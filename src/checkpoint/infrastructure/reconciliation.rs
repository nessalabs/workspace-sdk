use super::file_tree::*;
use super::filesystem::*;
use super::git_checkpoint_engine::GitCheckpointEngine;
use super::scanner::*;
use crate::checkpoint::domain::*;
use crate::{Error, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::str::from_utf8;
use uuid::Uuid;

impl GitCheckpointEngine {
    pub(super) fn load_reconciliation(
        &self,
        reference: &ReconciliationRef,
    ) -> Result<Reconciliation> {
        let result: Reconciliation = self.read_json(
            &self
                .reconciliation_dir(reference)?
                .join("reconciliation.json"),
        )?;
        if &result.reference != reference {
            return Err(Error::State("reconciliation reference mismatch".into()));
        }
        Ok(result)
    }

    pub(super) fn review_tree(&self, reconciliation: &Reconciliation) -> Result<Tree> {
        let mut tracked = BTreeSet::new();
        for reference in [
            &reconciliation.base,
            &reconciliation.local,
            &reconciliation.remote,
        ] {
            tracked.extend(
                self.load_checkpoint(reference)?
                    .manifest
                    .entries
                    .into_iter()
                    .map(|entry| entry.path),
            );
        }
        scan(&reconciliation.review_directory, &tracked, &self.limits)
    }

    pub(super) fn save_reconciliation(&self, value: &Reconciliation) -> Result<()> {
        self.write_json(
            &self
                .reconciliation_dir(&value.reference)?
                .join("reconciliation.json"),
            value,
        )
    }

    pub(super) fn tracked_allow_unmerged(&self, repo: &Path) -> Result<BTreeSet<String>> {
        let output = self.git(Some(repo), ["ls-files", "-z"], None)?;
        output
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| {
                from_utf8(p)
                    .map(str::to_owned)
                    .map_err(|_| Error::Unsupported("non-UTF-8 merge path".into()))
            })
            .collect()
    }

    pub(super) fn reconcile_operation(
        &self,
        base: &CheckpointRef,
        local: &Path,
        remote: &CheckpointRef,
    ) -> Result<Reconciliation> {
        let _lock = self.lock()?;
        let destination = self.checked_root(local)?;
        let base_tree = self.load_tree(base)?;
        let remote_tree = self.load_tree(remote)?;
        let local_checkpoint = self.capture_inner(
            &destination,
            CaptureOptions {
                parent: Some(base.clone()),
                ..Default::default()
            },
        )?;
        let local_tree = self.load_tree(local_checkpoint.reference())?;
        let reference = ReconciliationRef(Uuid::new_v4().to_string());
        let directory = self.reconciliation_dir(&reference)?;
        fs::create_dir(&directory)?;
        let originals_directory = directory.join("originals");
        fs::create_dir(&originals_directory)?;
        for (name, tree) in [
            ("base", &base_tree),
            ("local", &local_tree),
            ("remote", &remote_tree),
        ] {
            let root = originals_directory.join(name);
            fs::create_dir(&root)?;
            write_tree(&root, tree)?;
        }
        let repo = directory.join("merge");
        self.init_repo(&repo)?;
        let base_commit = self.commit_tree(&repo, &base_tree, None)?;
        let local_commit = self.commit_tree(&repo, &local_tree, Some(&base_commit))?;
        let remote_commit = self.commit_tree(&repo, &remote_tree, Some(&base_commit))?;
        self.git(
            Some(&repo),
            ["update-ref", "refs/heads/local", &local_commit],
            None,
        )?;
        self.git(
            Some(&repo),
            ["update-ref", "refs/heads/remote", &remote_commit],
            None,
        )?;
        self.git(
            Some(&repo),
            ["symbolic-ref", "HEAD", "refs/heads/local"],
            None,
        )?;
        self.git(Some(&repo), ["reset", "--hard", &local_commit], None)?;
        let merged = self.git_raw(
            Some(&repo),
            [
                "-c",
                "merge.renames=false",
                "merge",
                "--no-commit",
                "--no-ff",
                "--no-edit",
                &remote_commit,
            ],
            None,
        )?;
        if !merged.status.success() && merged.status.code() != Some(1) {
            return Err(Error::CommandFailed {
                message: format!(
                    "Git merge failed: {}",
                    String::from_utf8_lossy(&merged.stderr)
                ),
            });
        }
        let unmerged = self.git(Some(&repo), ["ls-files", "--unmerged", "-z"], None)?;
        let mut conflicts: BTreeMap<String, ConflictRecord> = BTreeMap::new();
        for record in unmerged.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            let text =
                from_utf8(record).map_err(|_| Error::State("non-UTF-8 conflict path".into()))?;
            let (header, path) = text
                .split_once('\t')
                .ok_or_else(|| Error::State("invalid conflict record".into()))?;
            let fields: Vec<_> = header.split_whitespace().collect();
            if fields.len() != 3 {
                return Err(Error::State("invalid conflict stage".into()));
            }
            let stage = fields[2];
            if !matches!(stage, "1" | "2" | "3") {
                return Err(Error::State("invalid merge stage".into()));
            }
            // D/F conflicts can have Git-generated stage paths (e.g. name~HEAD)
            // that do not exist in the original tree. Preserve the exact stage
            // blob independently, and retain the full originals alongside it.
            let conflict_objects = originals_directory.join("conflict-objects");
            fs::create_dir_all(&conflict_objects)?;
            let preserved_path = conflict_objects.join(format!("{}-{stage}", fields[1]));
            if !preserved_path.exists() {
                fs::write(
                    &preserved_path,
                    self.git(Some(&repo), ["cat-file", "blob", fields[1]], None)?,
                )?;
            }
            let record = conflicts
                .entry(path.into())
                .or_insert_with(|| ConflictRecord {
                    path: path.into(),
                    base: None,
                    local: None,
                    remote: None,
                });
            let side = ConflictSide {
                object_id: fields[1].into(),
                mode: fields[0]
                    .parse()
                    .map_err(|_| Error::State("invalid conflict mode".into()))?,
                preserved_path,
            };
            match stage {
                "1" => record.base = Some(side),
                "2" => record.local = Some(side),
                "3" => record.remote = Some(side),
                _ => unreachable!(),
            }
        }
        // Review is a regular directory with no Git metadata or executable hooks.
        // Git-generated conflict paths are retained; all originals remain intact.
        let review_directory = directory.join("review");
        fs::create_dir(&review_directory)?;
        let review_tree = scan(&repo, &self.tracked_allow_unmerged(&repo)?, &self.limits)?;
        write_tree(&review_directory, &review_tree)?;
        let conflicts: Vec<_> = conflicts.into_values().collect();
        let result = if conflicts.is_empty() {
            Some(
                self.save_tree(
                    &review_tree,
                    CaptureOptions {
                        parent: Some(base.clone()),
                        ..Default::default()
                    },
                )?
                .manifest
                .reference,
            )
        } else {
            None
        };
        let reconciliation = Reconciliation {
            reference,
            base: base.clone(),
            local: local_checkpoint.manifest.reference,
            remote: remote.clone(),
            destination,
            review_directory,
            originals_directory,
            expected_fingerprint: local_tree.fingerprint()?,
            result,
            status: if conflicts.is_empty() {
                ReconciliationStatus::StagedForReview
            } else {
                ReconciliationStatus::Conflicted
            },
            conflicts,
            recovery: None,
            applied: None,
        };
        self.save_reconciliation(&reconciliation)?;
        Ok(reconciliation)
    }

    pub(super) fn accept_resolution_operation(
        &self,
        reference: &ReconciliationRef,
        resolved_paths: &[String],
    ) -> Result<Reconciliation> {
        let _lock = self.lock()?;
        let mut reconciliation = self.load_reconciliation(reference)?;
        if reconciliation.status == ReconciliationStatus::Applied {
            return Err(Error::Conflict("reconciliation was already applied".into()));
        }
        if reconciliation.recovery.is_some() {
            return Err(Error::Conflict(
                "apply already started; finish or roll back recovery before staging a new resolution".into(),
            ));
        }
        let expected: BTreeSet<_> = reconciliation
            .conflicts
            .iter()
            .map(|c| c.path.as_str())
            .collect();
        let acknowledged: BTreeSet<_> = resolved_paths.iter().map(String::as_str).collect();
        if expected != acknowledged || acknowledged.len() != resolved_paths.len() {
            return Err(Error::InvalidInput(
                "explicitly acknowledge each conflict path exactly once".into(),
            ));
        }
        let tree = self.review_tree(&reconciliation)?;
        if self.review_tree(&reconciliation)?.fingerprint()? != tree.fingerprint()? {
            return Err(Error::Conflict(
                "review files changed during capture".into(),
            ));
        }
        reconciliation.result = Some(
            self.save_tree(
                &tree,
                CaptureOptions {
                    parent: Some(reconciliation.base.clone()),
                    ..Default::default()
                },
            )?
            .manifest
            .reference,
        );
        reconciliation.status = ReconciliationStatus::StagedForReview;
        self.save_reconciliation(&reconciliation)?;
        Ok(reconciliation)
    }
}
