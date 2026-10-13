use super::NativeCommand;
use crate::{ExecResult, Result};
use async_trait::async_trait;

#[async_trait]
/// Injected boundary for native executable invocation by infrastructure adapters.
///
/// Production applications normally use [`super::SystemCommandRunner`]. Custom
/// implementations can supply a controlled process launcher or deterministic
/// transport responses for tests without changing workspace behavior.
pub trait ICommandRunner: Send + Sync {
    /// Execute one command, respecting its transport timeout and sensitive input.
    ///
    /// A nonzero process exit is returned as an execution result. Failures to
    /// spawn, communicate, or finish within the transport budget return an error.
    /// Implementations must not automatically retry a command: a lost response
    /// can follow an already successful managed-run launch.
    async fn run(&self, command: NativeCommand) -> Result<ExecResult>;
}
