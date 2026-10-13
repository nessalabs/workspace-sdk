use std::{fmt, time::Duration};

/// One bounded invocation of a host executable through a [`super::ICommandRunner`].
///
/// Providers use this for their supervisor, SSH, or container-engine transport.
/// It is separate from a workload command: transport completion or disconnection
/// does not determine whether a detached managed run has finished.
/// `Debug` omits argument contents and standard input because they may be sensitive.
pub struct NativeCommand {
    /// Host executable name or path to spawn, without an implicit shell wrapper.
    pub program: String,
    /// Ordered arguments passed directly to the executable as separate values.
    ///
    /// Shell interpretation occurs only when the selected executable or adapter
    /// explicitly invokes a shell, as OpenSSH does for its remote command.
    pub args: Vec<String>,
    /// Bytes sent to standard input, followed by EOF.
    ///
    /// This transient payload may contain credentials and must not be logged.
    pub stdin: Vec<u8>,
    /// Maximum wall-clock duration allowed for this native transport invocation.
    ///
    /// This is independent of a managed workload's deadline. On expiration, a
    /// launch may already have succeeded remotely and must be reconciled by run ID.
    pub timeout: Duration,
}
impl fmt::Debug for NativeCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeCommand")
            .field("program", &self.program)
            .field("argument_count", &self.args.len())
            .field("stdin", &"<redacted>")
            .field("timeout", &self.timeout)
            .finish()
    }
}
