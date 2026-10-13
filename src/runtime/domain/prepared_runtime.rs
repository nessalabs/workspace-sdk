//! The checked, transient launch capability granted by successful preparation.
//!
//! Unlike persistable requests and receipts, this capability carries private resolved
//! state and can be constructed only by the preparation service.

use super::{
    AuthenticationState, PreparationReceipt, RuntimeCommand, RuntimeKind, RuntimeServerSpec,
    SecretValue,
};
use crate::{Error, Result};

/// An in-memory launch capability produced only by successful preparation.
///
/// Its fields are private and it is not deserializable, so an old
/// [`crate::runtime::PreparationReceipt`] cannot bypass current setup or authentication checks.
/// It combines the selected executable, environment, transient credentials, and
/// current authentication observation. It is not a running process or durable entity;
/// call preparation again before a later retry to resolve rotation or expiry.
#[derive(Clone, Debug)]
pub struct PreparedRuntime {
    pub(crate) receipt: PreparationReceipt,
    pub(crate) kind: RuntimeKind,
    pub(crate) base: RuntimeCommand,
    pub(crate) authentication: AuthenticationState,
}

impl PreparedRuntime {
    /// Borrow the nonsecret progress snapshot associated with this prepared capability.
    /// The snapshot can be persisted, but cannot reconstruct this capability.
    pub fn receipt(&self) -> &PreparationReceipt {
        &self.receipt
    }
    /// Borrow the authentication observation made while producing this capability.
    /// It describes local checks and does not guarantee a future upstream model request.
    pub fn authentication(&self) -> &AuthenticationState {
        &self.authentication
    }
    /// Build a workload command using the prepared executable and environment.
    ///
    /// The supplied arguments replace the base argument vector and are passed directly
    /// to the executable. This only builds a command; the caller chooses when and how
    /// the [`crate::runtime::IRuntimeHost`] executes it. The base command has no execution timeout.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] if an argument contains a NUL byte.
    /// Credentials remain transient in-memory material; prepare again when retrying later.
    pub fn launch_command(
        &self,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<RuntimeCommand> {
        let mut command = self.base.clone();
        command.args = args.into_iter().map(Into::into).collect();
        if command.args.iter().any(|arg| arg.contains('\0')) {
            return Err(Error::InvalidInput("argument contains NUL".into()));
        }
        Ok(command)
    }

    /// Build a password-protected OpenCode HTTP server command bound to `127.0.0.1`.
    ///
    /// The password travels through the secret environment and the username is
    /// `opencode`. The returned [`crate::runtime::RuntimeServerSpec`] includes the health route
    /// and requires the caller to obtain an authenticated backend endpoint after launch.
    /// This helper neither starts the server nor exposes its port.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Unsupported`] for other runtime kinds, or
    /// [`crate::Error::InvalidInput`] for port zero, an empty password, or a NUL byte.
    pub fn opencode_server(&self, port: u16, password: SecretValue) -> Result<RuntimeServerSpec> {
        if !matches!(self.kind, RuntimeKind::OpenCode { .. }) {
            return Err(Error::Unsupported("server recipe requires OpenCode".into()));
        }
        if port == 0 || password.is_empty() || password.expose().contains('\0') {
            return Err(Error::InvalidInput(
                "server requires a nonzero port and password".into(),
            ));
        }
        let mut command = self.launch_command([
            "serve",
            "--hostname",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ])?;
        command
            .secret_environment
            .insert("OPENCODE_SERVER_PASSWORD".into(), password);
        command
            .environment
            .insert("OPENCODE_SERVER_USERNAME".into(), "opencode".into());
        Ok(RuntimeServerSpec {
            command,
            port,
            health_path: "/global/health".into(),
            requires_authenticated_endpoint: true,
        })
    }
}
