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
    pub fn new(
        git_executable: impl Into<PathBuf>,
        state_directory: impl Into<PathBuf>,
    ) -> Result<Self> {
        let state = state_directory.into();
        fs::create_dir_all(&state)?;
        if fs::symlink_metadata(&state)?.file_type().is_symlink() {
            return Err(Error::InvalidInput(
                "checkpoint state cannot be a symlink".into(),
            ));
        }
        #[cfg(unix)]
        {
            fs::set_permissions(&state, fs::Permissions::from_mode(0o700))?;
        }
        let engine = Self {
            git_executable: git_executable.into(),
            state_directory: fs::canonicalize(state)?,
            limits: CheckpointLimits::default(),
        };
        engine.git(None, ["--version"], None)?;
        for directory in ["checkpoints", "reconciliations", "recoveries"] {
            fs::create_dir_all(engine.state_directory.join(directory))?;
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
