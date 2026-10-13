use super::filesystem::*;
use super::git_checkpoint_engine::GitCheckpointEngine;
use super::path_policy::*;
use crate::checkpoint::domain::*;
use crate::{Error, Result};
use fs2::FileExt;
use serde::{de::DeserializeOwned, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

impl GitCheckpointEngine {
    pub(super) fn lock(&self) -> Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.state_directory.join("engine.lock"))?;
        file.lock_exclusive()?;
        Ok(file)
    }

    pub(super) fn checked_root(&self, root: &Path) -> Result<PathBuf> {
        if fs::symlink_metadata(root)?.file_type().is_symlink() {
            return Err(Error::InvalidInput(
                "project root cannot be a symlink".into(),
            ));
        }
        let root = fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(Error::InvalidInput(
                "project root must be a directory".into(),
            ));
        }
        if self.state_directory.starts_with(&root) || root == self.state_directory {
            return Err(Error::InvalidInput(
                "private checkpoint state must be outside the project".into(),
            ));
        }
        Ok(root)
    }

    pub(super) fn write_json<T: Serialize>(&self, path: &Path, value: &T) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(value)?;
        if bytes.len() as u64 > self.limits.max_manifest_bytes {
            return Err(Error::InvalidInput(
                "checkpoint metadata exceeds size limit".into(),
            ));
        }
        let parent = path
            .parent()
            .ok_or_else(|| Error::State("metadata parent missing".into()))?;
        let mut temporary = NamedTempFile::new_in(parent)?;
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        temporary.persist(path).map_err(|e| Error::Io(e.error))?;
        File::open(parent)?.sync_all()?;
        Ok(())
    }

    pub(super) fn read_json<T: DeserializeOwned>(&self, path: &Path) -> Result<T> {
        Ok(serde_json::from_slice(&read_regular(
            path,
            self.limits.max_manifest_bytes,
        )?)?)
    }

    pub(super) fn checkpoint_dir(&self, reference: &CheckpointRef) -> Result<PathBuf> {
        valid_id(&reference.0)?;
        Ok(self.state_directory.join("checkpoints").join(&reference.0))
    }

    pub(super) fn reconciliation_dir(&self, reference: &ReconciliationRef) -> Result<PathBuf> {
        valid_id(&reference.0)?;
        Ok(self
            .state_directory
            .join("reconciliations")
            .join(&reference.0))
    }

    pub(super) fn recovery_dir(&self, reference: &RecoveryRef) -> Result<PathBuf> {
        valid_id(&reference.0)?;
        Ok(self.state_directory.join("recoveries").join(&reference.0))
    }

    pub(super) fn empty_destination(&self, destination: &Path) -> Result<PathBuf> {
        if !destination.exists() {
            fs::create_dir_all(destination)?;
        }
        let destination = self.checked_root(destination)?;
        if fs::read_dir(&destination)?.next().is_some() {
            return Err(Error::Conflict("restore destination must be empty".into()));
        }
        Ok(destination)
    }
}
