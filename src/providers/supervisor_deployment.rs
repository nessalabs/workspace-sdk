//! Explicit selection of the native supervisor installed on a target.
use super::managed::validate_remote_path;
use crate::{Error, Result};
use std::{
    env,
    fs::{self, File},
    path::PathBuf,
};

/// Operating system and CPU architecture for a supplied supervisor artifact.
/// The artifact must also support the target's C runtime; static Linux builds are portable across more images.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TargetPlatform {
    LinuxX86_64,
    LinuxAarch64,
    MacosX86_64,
    MacosAarch64,
}
impl TargetPlatform {
    pub(super) fn verify_uname(&self, output: &str) -> Result<()> {
        let fields = output.split_whitespace().collect::<Vec<_>>();
        let matches = matches!(
            (fields.first().copied(), fields.get(1).copied(), self),
            (Some("Linux"), Some("x86_64"), Self::LinuxX86_64)
                | (Some("Linux"), Some("aarch64" | "arm64"), Self::LinuxAarch64)
                | (Some("Darwin"), Some("x86_64"), Self::MacosX86_64)
                | (
                    Some("Darwin"),
                    Some("arm64" | "aarch64"),
                    Self::MacosAarch64
                )
        );
        if matches {
            Ok(())
        } else {
            Err(Error::Unsupported(
                "supervisor artifact does not match the target operating system and architecture"
                    .into(),
            ))
        }
    }
}
/// A caller-supplied compiled Rust supervisor, tagged with its target platform.
#[derive(Clone, Debug)]
pub struct SupervisorArtifact {
    /// Local path of the binary to upload. This need not run on the controller's machine.
    pub path: PathBuf,
    /// Platform the binary was compiled for; checked before upload.
    pub platform: TargetPlatform,
}
impl SupervisorArtifact {
    pub(super) fn bytes(&self) -> Result<Vec<u8>> {
        let file = File::open(&self.path)?;
        if file.metadata()?.len() > 128 * 1024 * 1024 {
            return Err(Error::InvalidInput(
                "supervisor artifact exceeds 128 MiB".into(),
            ));
        }
        Ok(fs::read(&self.path)?)
    }
}
/// How a remote provider obtains its native supervisor. No host binary is copied implicitly.
#[derive(Clone, Debug)]
pub enum SupervisorDeployment {
    /// Use an already installed executable, resolved on the target.
    Preinstalled { executable: String },
    /// Upload an explicitly compatible artifact to an absolute target executable path.
    Upload {
        artifact: SupervisorArtifact,
        executable: String,
    },
}
impl Default for SupervisorDeployment {
    fn default() -> Self {
        Self::Preinstalled {
            executable: "workspace-supervisor".into(),
        }
    }
}
impl SupervisorDeployment {
    pub fn executable(&self) -> &str {
        match self {
            Self::Preinstalled { executable } | Self::Upload { executable, .. } => executable,
        }
    }
    pub(super) fn validate(&self) -> Result<()> {
        let executable = self.executable();
        if executable.is_empty() || executable.contains('\0') {
            return Err(Error::InvalidInput(
                "supervisor executable is required".into(),
            ));
        }
        if matches!(self, Self::Upload { .. }) {
            validate_remote_path(executable)?;
        }
        Ok(())
    }
}
pub(super) fn local_supervisor() -> PathBuf {
    if let Ok(current) = env::current_exe() {
        if let Some(parent) = current.parent() {
            let sibling = parent.join("workspace-supervisor");
            if sibling.is_file() {
                return sibling;
            }
        }
    }
    PathBuf::from("workspace-supervisor")
}
