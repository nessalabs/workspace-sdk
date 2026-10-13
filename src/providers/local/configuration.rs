use crate::providers::supervisor_deployment::local_supervisor;
use crate::{Error, Isolation, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Host execution settings shared by the workspaces of a [`super::LocalProvider`].
///
/// Local workloads run with the application's operating-system permissions.
/// Use [`Self::trusted_host`] to acknowledge that they can access the host;
/// a task directory does not provide a security sandbox.
#[derive(Clone, Debug)]
pub struct LocalConfig {
    /// Local namespace for SDK task directories and deletion receipts.
    ///
    /// New workspaces use `<base_dir>/<workspace_id>/project` for project files
    /// and the sibling `control` directory for private execution state.
    /// The provider creates and canonicalizes this directory when opening a task.
    pub base_dir: PathBuf,
    /// Native Rust supervisor executable used to manage workloads.
    ///
    /// The default is a sibling `workspace-supervisor`, or the host's executable search
    /// path. An absolute executable path may be supplied instead.
    pub supervisor: PathBuf,
    /// Required acknowledgement of host execution permissions.
    ///
    /// Only [`Isolation::TrustedHost`] is accepted by the local provider.
    pub isolation: Isolation,
}
impl LocalConfig {
    /// Configure host execution under `base_dir`, using the sibling or installed `workspace-supervisor` by default.
    pub fn trusted_host(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
            supervisor: local_supervisor(),
            isolation: Isolation::TrustedHost,
        }
    }

    pub(super) fn validate(&self) -> Result<()> {
        if self.isolation != Isolation::TrustedHost {
            return Err(Error::Unsupported("local provider requires explicit TrustedHost isolation; use ContainerProvider for container isolation".into()));
        }
        if self.supervisor.as_os_str().is_empty() {
            return Err(Error::InvalidInput("invalid supervisor executable".into()));
        }
        Ok(())
    }
}

/// Per-workspace options serialized into [`crate::OpenRequest`] `config`.
///
/// The default creates an SDK-owned project directory. Attachment uses an
/// existing project while keeping supervisor state under [`LocalConfig::base_dir`].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocalOpenOptions {
    /// Existing local project directory to attach to, resolved to its canonical path.
    ///
    /// `None` creates a new project under the provider's task namespace.
    /// Attached workspaces support file transfer and managed runs, but their
    /// workspace-level `stop` and `destroy` operations are disabled.
    pub attach: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_isolation_requires_explicit_host_trust() {
        let mut config = LocalConfig::trusted_host("/tmp/workspaces");
        assert!(config.validate().is_ok());
        config.isolation = Isolation::Container;
        assert!(matches!(config.validate(), Err(Error::Unsupported(_))));
    }
}
