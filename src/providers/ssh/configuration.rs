use crate::providers::managed;
use crate::providers::SupervisorDeployment;
use crate::{Error, Isolation, Result};
use serde::{Deserialize, Serialize};

/// Connection and task-directory settings for a [`super::SshProvider`].
///
/// Authentication comes from the caller's configured OpenSSH keys or agent.
/// Commands use batch authentication and strict known-host verification; the
/// provider does not create the host or provide a sandbox on it.
#[derive(Clone, Debug)]
pub struct SshConfig {
    /// OpenSSH host alias or host address, optionally including a user name.
    ///
    /// This is a host identifier such as `gpu-worker` or `user@host`, not a
    /// command string. Connection options belong in the caller's SSH configuration.
    pub host: String,
    /// Absolute, normalized POSIX base directory on the remote host.
    ///
    /// Each workspace gets `<root>/<workspace_id>/project` and a sibling
    /// `control` directory. Directory traversal through symlinks is rejected.
    pub root: String,
    /// Local OpenSSH-compatible executable; defaults to `ssh`.
    ///
    /// The provider supplies known-host checks, batch mode, and connection
    /// timeout arguments when invoking this executable.
    pub ssh_program: String,
    /// Explicit native supervisor deployment on the remote host.
    ///
    /// A name is resolved by the remote host, or an absolute target path may be used.
    pub supervisor: SupervisorDeployment,
    /// Required acknowledgement that commands run with the SSH user's permissions.
    ///
    /// Only [`Isolation::TrustedHost`] is accepted by the SSH provider.
    pub isolation: Isolation,
}
impl SshConfig {
    /// Configure an existing host and remote task namespace using `ssh` and a configured native supervisor.
    ///
    /// The caller configures keys, an agent, and known hosts before connecting.
    pub fn trusted_host(host: impl Into<String>, root: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            root: root.into(),
            ssh_program: "ssh".into(),
            supervisor: SupervisorDeployment::default(),
            isolation: Isolation::TrustedHost,
        }
    }

    pub(super) fn validate(&self) -> Result<()> {
        validate_host(&self.host)?;
        managed::validate_remote_path(&self.root)?;
        if self.isolation != Isolation::TrustedHost {
            return Err(Error::Unsupported(
                "SSH adapter requires explicit TrustedHost isolation".into(),
            ));
        }
        if self.ssh_program.is_empty() || self.ssh_program.contains('\0') {
            return Err(Error::InvalidInput("invalid SSH executable".into()));
        }
        self.supervisor.validate()?;
        Ok(())
    }
}

fn validate_host(value: &str) -> Result<()> {
    if value.is_empty()
        || value.starts_with('-')
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"@._-:[]".contains(&b))
    {
        return Err(Error::InvalidInput(
            "SSH host must be a host name or configured alias, without command options".into(),
        ));
    }
    Ok(())
}

/// Options serialized into [`crate::AttachRequest`] `config` for an existing
/// remote project.
///
/// Creating an SSH workspace takes no options: it makes
/// `<SshConfig::root>/<workspace_id>/project` and does not provision another
/// host. An attachment keeps its private control directory under the
/// provider's task namespace and cannot stop or destroy the project.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SshAttachOptions {
    /// Existing absolute, normalized remote project directory.
    pub path: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ssh_rejects_options_and_untrusted_isolation_claims() {
        assert!(
            SshConfig::trusted_host("-oProxyCommand=evil", "/srv/workspaces")
                .validate()
                .is_err()
        );
        let mut config = SshConfig::trusted_host("host", "/srv/workspaces");
        config.isolation = Isolation::ProviderSandbox;
        assert!(config.validate().is_err());
    }
}
