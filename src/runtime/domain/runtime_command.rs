//! Resolved command plans, foreground output, and protected server launch contracts.
//!
//! Commands and server specifications describe execution intent. Output records a
//! completed invocation. These values do not own a process lifecycle or persist
//! resolved credential material.

use super::SecretValue;
use std::collections::BTreeMap;
use std::fmt;

/// A resolved target invocation whose environment may contain transient secrets.
///
/// This is a command value, not a running process or persistence record. It deliberately
/// has no serialization implementation. [`crate::runtime::IRuntimeHost`] adapters must keep
/// credential inputs out of durable command records and diagnostics while preserving
/// argument boundaries, environment removal, working directory, and timeout behavior.
#[derive(Clone, Debug)]
pub struct RuntimeCommand {
    /// Executable name or target path; no implicit shell interprets the string.
    pub program: String,
    /// Ordered argument values. Applications must not place secrets in these ordinary
    /// arguments; use the secret environment or the host's private stdin channel.
    pub args: Vec<String>,
    /// Absolute target working directory selected by the preparation request.
    pub working_directory: String,
    /// Ordinary variables, including the SDK-selected private auth directory settings.
    /// These values are not treated as secret material.
    pub environment: BTreeMap<String, String>,
    /// Resolved credential bindings supplied through the host's private execution
    /// transport. Their [`crate::runtime::SecretValue`] debug representation is redacted.
    pub secret_environment: BTreeMap<String, SecretValue>,
    /// Inherited variable names to remove before applying this command's environment.
    /// This prevents ambient credentials or routing settings from changing the explicit
    /// authentication choice.
    pub remove_environment: Vec<String>,
    /// Execution timeout in seconds. Zero means no timeout for a prepared workload;
    /// recipe probes, installation, and login commands use bounded positive values.
    pub timeout_seconds: u64,
}

/// Transient output from a completed runtime-host command.
///
/// Exit status can establish probe success; transport failure is represented by
/// [`crate::Error`] instead of a synthetic failed probe. Debug output omits stream
/// contents, and preparation receipts never persist native status/login output.
/// This is not an output-redaction layer: a workload can still print its own secrets.
#[derive(Clone, Default)]
pub struct RuntimeOutput {
    /// Process result reported by the host. Zero conventionally indicates success;
    /// nonzero can mean a missing executable or a failed probe/application command.
    pub exit_code: i32,
    /// Captured standard output, used transiently for version or native status parsing.
    pub stdout: String,
    /// Captured standard error. It may contribute to native status/version parsing
    /// but is not included in this type's debug representation.
    pub stderr: String,
}

impl fmt::Debug for RuntimeOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeOutput")
            .field("exit_code", &self.exit_code)
            .finish_non_exhaustive()
    }
}

/// A prepared HTTP-server launch plan with its expected access contract.
///
/// This value does not represent a started server or a reachable endpoint. Launch
/// [`Self::command`] through the host, then ask the execution backend for port access
/// that satisfies the required policy.
#[derive(Clone, Debug)]
pub struct RuntimeServerSpec {
    /// Resolved server invocation, including transient authentication material.
    pub command: RuntimeCommand,
    /// Target TCP listening port. [`crate::runtime::PreparedRuntime::opencode_server`] rejects
    /// zero and binds the server on target loopback.
    pub port: u16,
    /// Relative HTTP route for a caller's readiness check after launch, such as
    /// `/global/health`; constructing this value does not perform the check.
    pub health_path: String,
    /// Whether externally reachable access must use the backend's authenticated
    /// endpoint policy. A server password alone does not satisfy this access contract.
    pub requires_authenticated_endpoint: bool,
}
