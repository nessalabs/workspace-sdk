//! Run identity values, launch commands, and observed lifecycle state.
use super::ExecutionRequest;
use crate::domain::identifier::identifier;
use crate::domain::workspace::WorkspaceRef;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

identifier!(
    /// Stable identity of a managed process within a workspace.
    ///
    /// Reuse this ID for retries of the same launch; use [`RunRef`] when a handle
    /// must also carry the workspace needed for reconnecting.
    RunId
);

#[cfg(test)]
mod run_id_tests {
    use super::*;
    #[test]
    fn validates_direct_and_deserialized_ids() {
        for value in ["", ".", "..", "../unsafe", "space name"] {
            assert!(RunId::new(value).is_err());
            assert!(serde_json::from_value::<RunId>(serde_json::json!(value)).is_err());
        }
        assert_eq!(RunId::new("valid-id_1").unwrap().as_str(), "valid-id_1");
    }
}

/// A reconnectable handle combining a run identity with its workspace.
///
/// [`RunId`] names a run within a workspace; this value supplies the workspace
/// and provider needed to find it after the original client has exited. It
/// describes a process, not an agent conversation or a copy of process memory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRef {
    /// Workspace in which the process was launched.
    pub workspace: WorkspaceRef,
    /// Stable run identity within that workspace.
    pub id: RunId,
}

/// Observed lifecycle state of a managed process.
///
/// `Lost` means the outcome cannot be established, not that the process has
/// stopped. Handoff must require a confirmed terminal state to avoid two writers.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RunState {
    /// Launch has been recorded but the process is not yet confirmed running.
    Starting,
    /// The managed process is running.
    Running,
    /// The process finished and its exit code was recorded.
    Exited {
        /// Recorded process status code; zero conventionally denotes success.
        exit_code: i32,
    },
    /// Cancellation completed and the managed process was stopped.
    Canceled,
    /// The managed process was stopped after its deadline expired.
    TimedOut,
    /// The supervisor cannot establish the process outcome.
    Lost {
        /// Explanation of why the supervisor cannot establish the outcome.
        reason: String,
    },
}
impl RunState {
    /// Whether the process has a confirmed stopped state: exited, canceled, or timed out.
    /// An unknown outcome (`Lost`) is intentionally not considered terminal.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Exited { .. } | Self::Canceled | Self::TimedOut)
    }
    /// Whether the process is known to be starting or running.
    /// False does not prove it stopped: a `Lost` process has an unknown outcome.
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Starting | Self::Running)
    }
}

#[cfg(test)]
mod run_state_tests {
    use super::*;
    #[test]
    fn unknown_outcome_is_not_safe_to_handoff() {
        assert!(!RunState::Lost {
            reason: "supervisor missing".into()
        }
        .is_terminal());
        assert!(RunState::Canceled.is_terminal());
    }
}

/// A request to start a durable, reconnectable managed process.
///
/// Reuse the same [`RunId`] when retrying the same launch. Backends use that
/// identity to prevent duplicate starts and reject incompatible reuse.
#[derive(Clone, Debug)]
pub struct LaunchRequest {
    /// Stable identity for this launch within its workspace.
    pub id: RunId,
    /// Command and transient secrets to execute.
    pub execution: ExecutionRequest,
    /// Total retained stdout/stderr byte budget used by the managed supervisor.
    /// Built-in backends require between 2 bytes and 16 MiB inclusive.
    pub output_limit_bytes: u64,
}
