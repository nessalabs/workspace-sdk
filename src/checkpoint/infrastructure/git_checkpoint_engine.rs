use super::filesystem::{create_checkpoint_directory_with, PublicationStep};
use crate::checkpoint::domain::*;
use crate::{Error, Result};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Git is an injected executable, state is caller-owned, and no global registry
/// or source-repository mutation is needed. Keep `state_directory` outside every
/// captured project root. State contains preserved file contents; protect it as
/// application data and retain recovery journals until callers dispose of them.
#[derive(Clone, Debug)]
pub struct GitCheckpointEngine {
    pub(super) git_executable: PathBuf,
    pub(super) state_directory: PathBuf,
    pub(super) limits: CheckpointLimits,
}

impl GitCheckpointEngine {
    /// Create private checkpoint storage using the injected Git executable.
    ///
    /// Creates the state directory if absent, checks Git availability, and uses
    /// owner-only permissions on Unix. The directory must be outside every
    /// captured project; capture/restore/apply reject overlapping state paths.
    /// File and directory sync must be supported by the storage filesystem.
    /// Canonical directory ancestry is synced through the filesystem root,
    /// including when retrying previously interrupted directory creation.
    pub fn new(
        git_executable: impl Into<PathBuf>,
        state_directory: impl Into<PathBuf>,
    ) -> Result<Self> {
        Self::new_with_publication(git_executable, state_directory, PublicationStep::perform)
    }

    fn new_with_publication(
        git_executable: impl Into<PathBuf>,
        state_directory: impl Into<PathBuf>,
        mut perform: impl FnMut(&PublicationStep) -> Result<()>,
    ) -> Result<Self> {
        let state = create_checkpoint_directory_with(&state_directory.into(), &mut perform)?;
        #[cfg(unix)]
        {
            fs::set_permissions(&state, fs::Permissions::from_mode(0o700))?;
        }
        let engine = Self {
            git_executable: git_executable.into(),
            state_directory: state,
            limits: CheckpointLimits::default(),
        };
        engine.git(None, ["--version"], None)?;
        for directory in ["checkpoints", "reconciliations", "recoveries"] {
            create_checkpoint_directory_with(
                &engine.state_directory.join(directory),
                &mut perform,
            )?;
        }
        Ok(engine)
    }

    /// Replace this engine's resource bounds, rejecting any zero-valued limit.
    ///
    /// Remote supervisors continue to use the transport's default bounds.
    pub fn with_limits(mut self, limits: CheckpointLimits) -> Result<Self> {
        if limits.max_files == 0
            || limits.max_file_bytes == 0
            || limits.max_total_bytes == 0
            || limits.max_bundle_bytes == 0
            || limits.max_manifest_bytes == 0
        {
            return Err(Error::InvalidInput(
                "checkpoint limits must be positive".into(),
            ));
        }
        self.limits = limits;
        Ok(self)
    }

    /// Canonical private storage location containing snapshots and recovery data.
    pub fn state_directory(&self) -> &Path {
        &self.state_directory
    }

    /// Effective local capture, archive, bundle, and metadata bounds.
    pub fn limits(&self) -> &CheckpointLimits {
        &self.limits
    }
}

#[cfg(test)]
mod creation_tests {
    use super::*;
    use crate::checkpoint::infrastructure::filesystem::PublicationStep;

    #[test]
    fn constructor_failure_and_retry_do_not_skip_existing_unsynced_ancestors() {
        let root = tempfile::tempdir().unwrap();
        let anchor = fs::canonicalize(root.path()).unwrap();
        let state = anchor.join("new/state");
        let mut injected = false;
        assert!(
            GitCheckpointEngine::new_with_publication("git", &state, |step| {
                if matches!(step, PublicationStep::SyncDirectory(path) if path == &anchor) {
                    injected = true;
                    return Err(Error::State(
                        "injected constructor ancestor sync failure".into(),
                    ));
                }
                step.perform()
            })
            .is_err()
        );
        assert!(injected && state.is_dir());
        let mut anchor_retried = false;
        let mut root_synced = false;
        let engine = GitCheckpointEngine::new_with_publication("git", &state, |step| {
            if let PublicationStep::SyncDirectory(path) = step {
                anchor_retried |= path == &anchor;
                root_synced |= path.parent().is_none();
            }
            step.perform()
        })
        .unwrap();
        assert!(anchor_retried && root_synced);
        assert!(engine.state_directory.join("checkpoints").is_dir());
    }
}
