use super::file_tree::*;
use super::filesystem::*;
use super::git_checkpoint_engine::GitCheckpointEngine;
use super::path_policy::*;
use super::scanner::*;
use crate::checkpoint::domain::*;
use crate::{Error, Result};
use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::str::from_utf8;

impl GitCheckpointEngine {
    pub(super) fn tracked(&self, root: &Path) -> Result<BTreeSet<String>> {
        let probe = self.git_raw(Some(root), ["rev-parse", "--is-inside-work-tree"], None)?;
        if probe.status.success() && probe.stdout != b"true\n" && probe.stdout != b"false\n" {
            return Err(Error::State(
                "unexpected source Git worktree probe result".into(),
            ));
        }
        if !probe.status.success() || probe.stdout == b"false\n" {
            if has_git_marker(root)? {
                return Err(Error::State(
                    "source Git metadata is unavailable; refusing incomplete file selection".into(),
                ));
            }
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

/// A failed Git probe cannot turn a recognizable worktree into a plain directory.
/// Include ancestor markers for subdirectory captures and use no-follow metadata
/// so a dangling marker symlink is still evidence of unavailable Git state.
fn has_git_marker(root: &Path) -> Result<bool> {
    for directory in root.ancestors() {
        match fs::symlink_metadata(directory.join(".git")) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_detection_includes_ancestors_and_any_existing_entry_kind() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("project/nested");
        std::fs::create_dir_all(&root).unwrap();
        assert!(!has_git_marker(&root).unwrap());
        let marker = root.parent().unwrap().join(".git");
        std::fs::write(&marker, b"invalid pointer").unwrap();
        assert!(has_git_marker(&root).unwrap());
        std::fs::remove_file(&marker).unwrap();
        std::fs::create_dir(&marker).unwrap();
        assert!(has_git_marker(&root).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn dangling_git_marker_symlink_still_prevents_non_git_fallback() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::os::unix::fs::symlink("missing-metadata", root.join(".git")).unwrap();
        assert!(has_git_marker(&root).unwrap());
    }
}
