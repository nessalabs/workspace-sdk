use super::*;
use crate::Error;
use crate::Result;
use crate::{OutputCursor, OutputPage, SecretInput};
use async_trait::async_trait;

/// Anti-corruption boundary between preparation and an execution backend.
/// Implementations must exclude supplied credentials from diagnostic logs and
/// durable command records, honor environment removal, and enforce timeouts.
/// Workload output is separately retained; a workload must not print its secrets.
#[async_trait]
pub trait IRuntimeHost: Send + Sync {
    /// Stable exact provider/target namespace used in receipts and process refs.
    /// Equal human-readable target IDs on distinct hosts must have different scopes.
    fn scope_id(&self) -> Result<String>;
    /// Reject a preparation for any other target before performing host I/O.
    fn validate_target(&self, target_id: &str) -> Result<()>;
    async fn inspect_resources(&self) -> Result<AvailableResources>;
    async fn exec(&self, command: RuntimeCommand) -> Result<RuntimeOutput>;

    /// Native key login needs stdin. Material must travel through the backend's
    /// private transport, never a serialized command or diagnostic log.
    async fn exec_with_secret_stdin(
        &self,
        _command: RuntimeCommand,
        _input: &SecretValue,
    ) -> Result<RuntimeOutput> {
        Err(Error::Unsupported(
            "runtime host cannot supply private native-login stdin".into(),
        ))
    }

    /// The idempotency key identifies one login attempt. Repeated calls must
    /// reconnect to the same process; a native terminal may be required.
    async fn start_interactive(
        &self,
        _idempotency_key: &str,
        _command: RuntimeCommand,
        _requires_terminal: bool,
    ) -> Result<RuntimeProcessRef> {
        Err(Error::Unsupported(
            "runtime host cannot start a native login interaction".into(),
        ))
    }

    async fn inspect_process(&self, _process: &RuntimeProcessRef) -> Result<RuntimeProcessState> {
        Err(Error::Unsupported(
            "runtime host cannot inspect login processes".into(),
        ))
    }

    /// Read at most `limit` combined raw bytes (2..=128 KiB); cursors count raw
    /// bytes. Lossy UTF-8 decoding may expand text to three times the raw size
    /// and replace characters split across pages. PTY adapters place the merged stream in
    /// stdout. This is an authentication transcript, not terminal emulation state.
    /// Validate exact host scope before I/O and do not reset cursors on reconnect.
    async fn read_process_output(
        &self,
        _process: &RuntimeProcessRef,
        _cursor: OutputCursor,
        _limit: u32,
    ) -> Result<OutputPage> {
        Err(Error::Unsupported("native login output".into()))
    }

    /// Submit transient bytes to a live native login. Never persist/log the input
    /// or replay it automatically. Errors must establish that no bytes were
    /// accepted; ambiguous/partial writes use RuntimeInputOutcome instead.
    /// Dropping this future may not revoke input already accepted by the child.
    async fn write_process_input(
        &self,
        _process: &RuntimeProcessRef,
        _input: &SecretInput,
    ) -> Result<RuntimeInputOutcome> {
        Err(Error::Unsupported(
            "native login input requires terminal support".into(),
        ))
    }

    /// Resize the current native terminal, after validating its exact host scope.
    async fn resize_process(
        &self,
        _process: &RuntimeProcessRef,
        _size: RuntimeTerminalSize,
    ) -> Result<()> {
        Err(Error::Unsupported(
            "native login resize requires terminal support".into(),
        ))
    }

    /// Request cancellation of the owned login and report observed process state.
    /// On error or cancellation of this future, the receipt remains valid and the
    /// same process must be inspected; no replacement may be assumed safe.
    async fn cancel_process(&self, _process: &RuntimeProcessRef) -> Result<RuntimeProcessState> {
        Err(Error::Unsupported("native login cancellation".into()))
    }

    /// Create a private directory (0700 on POSIX), rejecting symlink traversal.
    async fn ensure_private_directory(&self, path: &str) -> Result<()>;

    /// Transport privately, write atomically with 0600 mode, and reject symlink
    /// traversal. These files are outside code checkpoints and general file sync.
    async fn provision_private_file(&self, _path: &str, _value: &SecretValue) -> Result<()> {
        Err(Error::Unsupported(
            "runtime host cannot privately provision credentials".into(),
        ))
    }
}
