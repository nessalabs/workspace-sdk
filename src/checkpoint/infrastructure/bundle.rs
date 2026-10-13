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
        Ok(CheckpointBundle {
            manifest: checkpoint.manifest,
            bytes,
        })
    }

    pub(super) fn import_operation(&self, bundle: &CheckpointBundle) -> Result<Checkpoint> {
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
        fs::rename(temporary.path(), final_directory)?;
        Ok(checkpoint)
    }
}
