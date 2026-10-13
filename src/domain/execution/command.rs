//! Process commands and their execution results; transient secrets stay outside serializable configuration.
use crate::domain::workspace::WorkspacePath;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};

/// Serializable configuration for a process executed inside a workspace.
///
/// Arguments are passed as an argument vector, not implicitly interpreted by a
/// shell. Use an explicit shell executable when shell syntax is intended. Put
/// credentials in [`ExecutionRequest::secrets`], not this serializable value.
/// `Debug` omits argument, environment-value, and stdin contents; serialization does not.
#[derive(Clone, Serialize, Deserialize)]
pub struct CommandSpec {
    /// Executable name or target-machine executable path.
    pub program: String,
    /// Ordered arguments passed to the executable.
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory relative to the workspace project root.
    #[serde(default)]
    pub cwd: WorkspacePath,
    /// Non-secret environment values added to the child process.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Inherited environment variables to remove before launch.
    #[serde(default)]
    pub env_remove: Vec<String>,
    /// Requested process deadline in seconds, measured from process launch.
    ///
    /// `None` leaves the deadline to the backend. Built-in foreground execution
    /// defaults to 60 seconds; a managed launch can run without a deadline.
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
    /// Optional input bytes. These are serializable, so avoid persistent secrets here.
    #[serde(default)]
    pub stdin: Option<Vec<u8>>,
}
impl fmt::Debug for CommandSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandSpec")
            .field("program", &self.program)
            .field("argument_count", &self.args.len())
            .field("cwd", &self.cwd)
            .field("environment_keys", &self.env.keys().collect::<Vec<_>>())
            .field("env_remove", &self.env_remove)
            .field("timeout_seconds", &self.timeout_seconds)
            .field("stdin", &self.stdin.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}
impl CommandSpec {
    /// Build a command with no arguments, environment overrides, input, or deadline.
    /// The initial working directory is the workspace project root.
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: vec![],
            cwd: WorkspacePath::root(),
            env: BTreeMap::new(),
            env_remove: vec![],
            timeout_seconds: None,
            stdin: None,
        }
    }
    /// Replace the argument vector without interpreting shell syntax.
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }
    /// Set a lexically validated directory relative to the project root.
    /// The backend checks filesystem and symlink boundaries at execution time.
    pub fn cwd(mut self, path: impl Into<String>) -> Result<Self> {
        self.cwd = WorkspacePath::new(path)?;
        Ok(self)
    }
    /// Set the requested process deadline in seconds; zero requests immediate timeout.
    pub fn timeout(mut self, seconds: u64) -> Self {
        self.timeout_seconds = Some(seconds);
        self
    }
    /// Reject an empty program, NUL-bearing program/arguments, and invalid environment names.
    ///
    /// This is structural validation. It does not check executable availability,
    /// environment values, resource policy, or whether the directory exists.
    pub fn validate(&self) -> Result<()> {
        if self.program.is_empty()
            || self.program.contains('\0')
            || self.args.iter().any(|v| v.contains('\0'))
        {
            return Err(Error::InvalidInput(
                "program and arguments must not contain NUL".into(),
            ));
        }
        for key in self.env.keys().chain(self.env_remove.iter()) {
            if key.is_empty() || key.contains('=') || key.contains('\0') {
                return Err(Error::InvalidInput(
                    "invalid environment variable name".into(),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod command_spec_tests {
    use super::*;
    #[test]
    fn debug_never_prints_argument_environment_or_stdin_content() {
        let mut command = CommandSpec::new("tool").args(["sensitive-argument"]);
        command
            .env
            .insert("TOKEN".into(), "sensitive-environment".into());
        command.stdin = Some(b"sensitive-input".to_vec());
        let text = format!("{command:?}");
        assert!(!text.contains("sensitive"));
    }
    #[test]
    fn refuses_invalid_program_and_environment_names() {
        assert!(CommandSpec::new("").validate().is_err());
        let mut command = CommandSpec::new("workload");
        command.env.insert("BAD=KEY".into(), "value".into());
        assert!(command.validate().is_err());
    }
}

/// A command paired with transient credentials at the execution boundary.
///
/// Unlike [`CommandSpec`], this request is deliberately not serializable. Resolve
/// secrets just before calling a backend; do not persist them in workspace handles.
#[derive(Clone, Debug)]
pub struct ExecutionRequest {
    /// Non-secret process configuration.
    pub command: CommandSpec,
    /// Environment credentials passed to the process for this execution.
    pub secrets: SecretEnvironment,
    /// Optional transient standard input, excluded from serializable command state.
    /// Leave [`CommandSpec::stdin`] empty when supplying this input; built-in
    /// backends reject requests that provide both forms.
    pub private_stdin: Option<SecretInput>,
}
impl From<CommandSpec> for ExecutionRequest {
    fn from(command: CommandSpec) -> Self {
        Self {
            command,
            secrets: SecretEnvironment::default(),
            private_stdin: None,
        }
    }
}

/// Transient process input whose bytes must not appear in debug or persisted commands.
///
/// This value object is used for native CLI key input. It contains plaintext
/// bytes in memory, but deliberately implements neither serialization nor
/// content-printing `Debug`.
#[derive(Clone)]
pub struct SecretInput(Vec<u8>);

impl SecretInput {
    /// Wrap input bytes for delivery through the private execution boundary.
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Borrow plaintext bytes only when delivering input to the target process.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SecretInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretInput(<redacted>)")
    }
}

/// Transient environment credentials supplied to a process at launch.
///
/// Values are excluded from `Debug`, and this type intentionally does not
/// implement serialization. The map still contains plaintext strings in memory;
/// this is a logging/persistence boundary, not encrypted or zeroizing storage.
#[derive(Clone, Default)]
pub struct SecretEnvironment(
    /// Plaintext environment values keyed by process environment-variable name.
    pub BTreeMap<String, String>,
);
impl fmt::Debug for SecretEnvironment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretEnvironment")
            .field("keys", &self.0.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[cfg(test)]
mod secret_environment_tests {
    use super::*;
    #[test]
    fn debug_excludes_secret_values() {
        let mut s = SecretEnvironment::default();
        s.0.insert("KEY".into(), "private-value".into());
        assert!(!format!("{s:?}").contains("private-value"));
    }
}

/// Captured result of a foreground command execution.
///
/// Use a managed run when output must be polled or execution must outlive the
/// client connection. An absent exit code does not imply success.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecResult {
    /// Process exit code when one was observed.
    pub exit_code: Option<i32>,
    /// Captured standard output as text.
    pub stdout: String,
    /// Captured standard error as text.
    pub stderr: String,
    /// Whether the execution deadline was exceeded.
    pub timed_out: bool,
    /// Whether the backend omitted output because of its capture limit.
    #[serde(default)]
    pub output_truncated: bool,
}
impl ExecResult {
    /// Whether a zero exit code was observed before the deadline expired.
    /// Output truncation does not change the command's success status.
    pub fn success(&self) -> bool {
        self.exit_code == Some(0) && !self.timed_out
    }
}
