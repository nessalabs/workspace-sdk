use super::file_tree::*;
use super::filesystem::*;
use super::git_checkpoint_engine::GitCheckpointEngine;
use super::pack::preflight_pack;
use crate::checkpoint::domain::*;
use crate::{Error, Result};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use tempfile::tempdir_in;

impl GitCheckpointEngine {
    pub(super) fn export_operation(&self, reference: &CheckpointRef) -> Result<CheckpointBundle> {
        self.export_operation_with(reference, PublicationStep::perform)
    }

    fn export_operation_with(
        &self,
        reference: &CheckpointRef,
        mut perform: impl FnMut(&PublicationStep) -> Result<()>,
    ) -> Result<CheckpointBundle> {
        let _lock = self.lock()?;
        let checkpoint = self.load_checkpoint(reference)?;
        let bytes = read_regular(
            &self.checkpoint_dir(reference)?.join("snapshot.bundle"),
            self.limits.max_bundle_bytes,
        )?;
        if digest(&bytes) != checkpoint.manifest.bundle_sha256
            || bytes.len() as u64 != checkpoint.manifest.bundle_bytes
        {
            return Err(Error::State(
                "stored checkpoint bundle checksum mismatch".into(),
            ));
        }
        // A readable old artifact may follow an interrupted publication. Export
        // is also the readiness check before committing a recovery head, so
        // validate the cached tree and finish syncing before acknowledging it.
        let directory = self.checkpoint_dir(reference)?;
        self.tree_from_repo(&directory.join("repo"), &checkpoint)?;
        sync_checkpoint_directory_with(&directory, &self.limits, &mut perform)?;
        Ok(CheckpointBundle {
            manifest: checkpoint.manifest,
            bytes,
        })
    }

    pub(super) fn import_operation(&self, bundle: &CheckpointBundle) -> Result<Checkpoint> {
        self.import_operation_with(bundle, PublicationStep::perform)
    }

    fn import_operation_with(
        &self,
        bundle: &CheckpointBundle,
        mut perform: impl FnMut(&PublicationStep) -> Result<()>,
    ) -> Result<Checkpoint> {
        let _lock = self.lock()?;
        self.validate_manifest(&bundle.manifest)?;
        if bundle.bytes.len() as u64 > self.limits.max_bundle_bytes
            || bundle.bytes.len() as u64 != bundle.manifest.bundle_bytes
            || digest(&bundle.bytes) != bundle.manifest.bundle_sha256
        {
            return Err(Error::InvalidInput(
                "bundle length/checksum mismatch".into(),
            ));
        }
        // Require exactly one self-contained SHA-1 ref; prerequisites or extra
        // refs would import history that is outside this checkpoint's manifest.
        let expected_header = format!(
            "# v2 git bundle\n{} refs/heads/snapshot\n\n",
            bundle.manifest.commit
        );
        if !bundle.bytes.starts_with(expected_header.as_bytes()) {
            return Err(Error::InvalidInput(
                "bundle must contain one self-contained snapshot ref".into(),
            ));
        }
        preflight_pack(
            &bundle.bytes[expected_header.len()..],
            &bundle.manifest,
            &self.limits,
        )?;
        let checkpoint = Checkpoint {
            manifest: bundle.manifest.clone(),
        };
        let final_directory = self.checkpoint_dir(&bundle.manifest.reference)?;
        if final_directory.exists() {
            let existing = self.load_checkpoint(&bundle.manifest.reference)?;
            if serde_json::to_vec(&existing.manifest)? != serde_json::to_vec(&bundle.manifest)? {
                return Err(Error::Conflict(
                    "checkpoint identifier already exists with different contents".into(),
                ));
            }
            let stored = read_regular(
                &final_directory.join("snapshot.bundle"),
                self.limits.max_bundle_bytes,
            )?;
            if stored.len() as u64 != existing.manifest.bundle_bytes
                || digest(&stored) != existing.manifest.bundle_sha256
            {
                return Err(Error::State(
                    "stored checkpoint bundle checksum mismatch".into(),
                ));
            }
            self.tree_from_repo(&final_directory.join("repo"), &existing)?;
            sync_checkpoint_directory_with(&final_directory, &self.limits, &mut perform)?;
            return Ok(existing);
        }
        let temporary = tempdir_in(self.state_directory.join("checkpoints"))?;
        let repo = temporary.path().join("repo");
        self.init_repo(&repo)?;
        let path = temporary.path().join("snapshot.bundle");
        fs::write(&path, &bundle.bytes)?;
        self.git(
            Some(&repo),
            [OsStr::new("bundle"), OsStr::new("verify"), path.as_os_str()],
            None,
        )?;
        self.git(
            Some(&repo),
            [
                OsStr::new("bundle"),
                OsStr::new("unbundle"),
                path.as_os_str(),
            ],
            None,
        )?;
        let parents = self.git_text(
            &repo,
            ["show", "-s", "--format=%P", &checkpoint.manifest.commit],
            None,
        )?;
        if !parents.is_empty() {
            return Err(Error::InvalidInput(
                "snapshot commit must not contain source history".into(),
            ));
        }
        self.git(
            Some(&repo),
            [
                "update-ref",
                "refs/heads/snapshot",
                &checkpoint.manifest.commit,
            ],
            None,
        )?;
        let objects = self.git_text(
            &repo,
            [
                "cat-file",
                "--batch-all-objects",
                "--batch-check=%(objectname) %(objecttype) %(objectsize)",
            ],
            None,
        )?;
        let reachable = self
            .git_text(&repo, ["rev-list", "--objects", "--all"], None)?
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        let mut total = 0u64;
        let mut count = 0usize;
        for object in objects.lines() {
            let fields: Vec<_> = object.split_whitespace().collect();
            if fields.len() != 3 || !reachable.contains(fields[0]) {
                return Err(Error::InvalidInput(
                    "bundle has undeclared/unreachable objects".into(),
                ));
            }
            let size = fields[2]
                .parse::<u64>()
                .map_err(|_| Error::InvalidInput("invalid object size".into()))?;
            total = total
                .checked_add(size)
                .ok_or_else(|| Error::InvalidInput("bundle size overflow".into()))?;
            count += 1;
            if (fields[1] == "blob" && size > self.limits.max_file_bytes)
                || total
                    > self
                        .limits
                        .max_total_bytes
                        .saturating_add(self.limits.max_manifest_bytes)
                || count > self.limits.max_files.saturating_mul(4096).saturating_add(2)
            {
                return Err(Error::InvalidInput("expanded bundle exceeds limits".into()));
            }
        }
        self.tree_from_repo(&repo, &checkpoint)?;
        self.write_json(&temporary.path().join("manifest.json"), &checkpoint)?;
        publish_checkpoint_directory_with(
            temporary.path(),
            &final_directory,
            &self.limits,
            &mut perform,
        )?;
        Ok(checkpoint)
    }
}

#[cfg(test)]
mod publication_retry_tests {
    use super::*;
    use crate::checkpoint::ports::ICheckpointEngine;

    fn bundle(root: &std::path::Path) -> CheckpointBundle {
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("counter"), b"3").unwrap();
        let engine = GitCheckpointEngine::new("git", root.join("source-store")).unwrap();
        let checkpoint = engine.capture(&source, CaptureOptions::default()).unwrap();
        engine.export(checkpoint.reference()).unwrap()
    }

    #[test]
    fn actual_export_verifies_cache_and_refuses_sync_failure_without_changing_bytes() {
        let root = tempfile::tempdir().unwrap();
        let bundle = bundle(root.path());
        let engine = GitCheckpointEngine::new("git", root.path().join("export-store")).unwrap();
        engine.import(&bundle).unwrap();
        let parent = engine.state_directory.join("checkpoints");
        let mut injected = false;
        assert!(engine
            .export_operation_with(&bundle.manifest.reference, |step| {
                if matches!(step, PublicationStep::SyncDirectory(path) if path == &parent) {
                    injected = true;
                    return Err(Error::State("injected export parent sync failure".into()));
                }
                step.perform()
            })
            .is_err());
        assert!(injected);
        assert_eq!(
            engine.export(&bundle.manifest.reference).unwrap().bytes,
            bundle.bytes
        );
        fs::remove_dir_all(
            engine
                .checkpoint_dir(&bundle.manifest.reference)
                .unwrap()
                .join("repo/.git/objects"),
        )
        .unwrap();
        assert!(engine.export(&bundle.manifest.reference).is_err());
    }

    #[test]
    fn exact_import_retry_verifies_and_syncs_after_uncertain_postrename_failure() {
        let root = tempfile::tempdir().unwrap();
        let bundle = bundle(root.path());
        let engine =
            GitCheckpointEngine::new("git", root.path().join("destination-store")).unwrap();
        let parent = engine.state_directory.join("checkpoints");
        let mut injected = false;
        assert!(engine
            .import_operation_with(&bundle, |step| {
                if matches!(step, PublicationStep::SyncDirectory(path) if path == &parent) {
                    injected = true;
                    return Err(Error::State("injected final parent sync".into()));
                }
                step.perform()
            })
            .is_err());
        assert!(injected);
        let destination = engine.checkpoint_dir(&bundle.manifest.reference).unwrap();
        assert!(destination.is_dir());
        let mut retried_parent = false;
        let mut bundle_synced = false;
        let retried = engine.import_operation_with(&bundle, |step| {
            if matches!(step, PublicationStep::SyncDirectory(path) if path == &parent) { retried_parent = true; }
            if matches!(step, PublicationStep::SyncFile(path) if path == &destination.join("snapshot.bundle")) { bundle_synced = true; }
            step.perform()
        }).unwrap();
        assert_eq!(retried.reference(), &bundle.manifest.reference);
        assert!(retried_parent && bundle_synced);
        let restored = root.path().join("restored");
        engine.restore(retried.reference(), &restored).unwrap();
        assert_eq!(fs::read(restored.join("counter")).unwrap(), b"3");
    }

    #[test]
    fn exact_retry_refuses_corrupt_bundle_or_missing_cached_git_objects() {
        for corrupt_bundle in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let bundle = bundle(root.path());
            let engine =
                GitCheckpointEngine::new("git", root.path().join("destination-store")).unwrap();
            engine.import(&bundle).unwrap();
            let destination = engine.checkpoint_dir(&bundle.manifest.reference).unwrap();
            if corrupt_bundle {
                fs::write(destination.join("snapshot.bundle"), b"corrupt").unwrap();
            } else {
                fs::remove_dir_all(destination.join("repo/.git/objects")).unwrap();
            }
            assert!(engine.import(&bundle).is_err());
        }
    }
}
