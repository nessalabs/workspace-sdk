use super::file_tree::*;
use super::filesystem::*;
use super::git_checkpoint_engine::GitCheckpointEngine;
use super::path_policy::*;
use super::scanner::*;
use crate::checkpoint::domain::*;
use crate::{Error, Result};
use std::collections::BTreeSet;
use std::path::Path;
use std::str::from_utf8;

impl GitCheckpointEngine {
    pub(super) fn tracked(&self, root: &Path) -> Result<BTreeSet<String>> {
        let probe = self.git_raw(Some(root), ["rev-parse", "--is-inside-work-tree"], None)?;
        if !probe.status.success() || probe.stdout != b"true\n" {
            return Ok(BTreeSet::new());
        }
        let output = self.git(Some(root), ["ls-files", "--stage", "-z", "--", "."], None)?;
        if output.len() as u64 > self.limits.max_manifest_bytes {
            return Err(Error::InvalidInput(
                "source Git index exceeds metadata limit".into(),
            ));
        }
        let mut tracked = BTreeSet::new();
        for record in output.split(|b| *b == 0).filter(|part| !part.is_empty()) {
            let record = from_utf8(record)
                .map_err(|_| Error::Unsupported("non-UTF-8 tracked path".into()))?;
            let (header, path) = record
                .split_once('\t')
                .ok_or_else(|| Error::State("invalid Git index record".into()))?;
            if header.starts_with("160000 ") {
                return Err(Error::Unsupported(format!(
                    "submodule requires explicit artifact/source handling: {path}"
                )));
            }
            if !header.ends_with(" 0") {
                return Err(Error::Conflict(format!(
                    "source Git index contains an unresolved merge: {path}"
                )));
            }
            relative_path(path)?;
            tracked.insert(path.to_owned());
        }
        Ok(tracked)
    }

    pub(super) fn scan(&self, root: &Path) -> Result<Tree> {
        scan(root, &self.tracked(root)?, &self.limits)
    }

    pub(super) fn capture_inner(&self, root: &Path, options: CaptureOptions) -> Result<Checkpoint> {
        let root = self.checked_root(root)?;
        let tree = self.scan(&root)?;
        if self.scan(&root)?.fingerprint()? != tree.fingerprint()? {
            return Err(Error::Conflict(
                "files changed during capture; stop writers and retry".into(),
            ));
        }
        self.save_tree(&tree, options)
    }

    pub(super) fn capture_operation(
        &self,
        root: &Path,
        options: CaptureOptions,
    ) -> Result<Checkpoint> {
        let _lock = self.lock()?;
        self.capture_inner(root, options)
    }

    pub(super) fn restore_operation(
        &self,
        reference: &CheckpointRef,
        destination: &Path,
    ) -> Result<RestoreReceipt> {
        let _lock = self.lock()?;
        let tree = self.load_tree(reference)?;
        validate_tree_symlinks(&tree)?;
        let destination = self.empty_destination(destination)?;
        write_tree(&destination, &tree)?;
        Ok(RestoreReceipt {
            checkpoint: reference.clone(),
            destination,
            fingerprint: tree.fingerprint()?,
        })
    }
}
