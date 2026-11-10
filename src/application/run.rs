use crate::{
    domain::{
        InputCloseOutcome, InputOutcome, OutputBytesPage, OutputCursor, OutputPage, RunRef,
        RunState, SecretInput,
    },
    ports::IBackend,
    Error, Result,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout_at, Instant};

/// An identity-bearing handle to one managed process in a workspace.
///
/// The identity remains stable while [`RunState`] changes. Dropping or cloning
/// this handle does not cancel or duplicate the process. Persist [`RunRef`] to
/// reconnect after restarting the caller.
#[derive(Clone)]
pub struct Run {
    pub(super) backend: Arc<dyn IBackend>,
    pub(super) reference: RunRef,
}
impl Run {
    /// Return the workspace-qualified identity needed to reconnect to this process.
    pub fn reference(&self) -> RunRef {
        self.reference.clone()
    }
    /// Observe the process lifecycle through its provider’s supervisor.
    pub async fn state(&self) -> Result<RunState> {
        self.backend.inspect_run(&self.reference).await
    }
    /// Read at most the requested output page size from absolute stream cursors.
    pub async fn output(&self, cursor: OutputCursor, limit: u32) -> Result<OutputPage> {
        self.backend
            .read_output(&self.reference, cursor, limit)
            .await
    }
    /// Read exact stdout/stderr bytes using independent absolute cursors.
    /// Built-in backends accept a combined page budget of 2..=262144 bytes.
    /// Reads do not advance server-side state; repeat or persist the returned
    /// cursors explicitly. Gaps must not be treated as complete protocol output.
    pub async fn output_bytes(&self, cursor: OutputCursor, limit: u32) -> Result<OutputBytesPage> {
        self.backend
            .read_output_bytes(&self.reference, cursor, limit)
            .await
    }
    /// Submit at most 64 KiB to an explicitly piped run once, without replay.
    /// An accepted prefix means kernel pipe admission, not child consumption.
    /// Submit only an unaccepted suffix after a known short result. Unknown
    /// delivery must not be resubmitted. Aborting this future cannot revoke bytes.
    pub async fn write_input(&self, input: &SecretInput) -> Result<InputOutcome> {
        if input.expose().is_empty() || input.expose().len() > 65536 {
            return Err(Error::InvalidInput(
                "managed input must contain 1..=65536 bytes".into(),
            ));
        }
        self.backend.write_input(&self.reference, input).await
    }
    /// Close this piped run's input with real EOF; this is idempotent.
    /// Await a write's acknowledgement before close when ordering is required.
    /// Concurrent callers are ordered by native dispatch. Unknown close can be
    /// retried on this same RunRef; it never permits replay of submitted bytes.
    /// Closed is not process termination or a successful cleanup acknowledgement.
    pub async fn close_input(&self) -> Result<InputCloseOutcome> {
        self.backend.close_input(&self.reference).await
    }
    /// Cancel this managed process group and observe the resulting state.
    pub async fn cancel(&self) -> Result<RunState> {
        self.backend.cancel(&self.reference).await
    }
    /// Poll until a confirmed terminal state or the caller’s waiting deadline.
    /// A waiting timeout leaves the process running; an unknown/lost state returns
    /// an error rather than treating the process as safely stopped.
    pub async fn wait(&self, timeout: Duration) -> Result<RunState> {
        let deadline = Instant::now().checked_add(timeout).ok_or_else(|| {
            Error::InvalidInput("run wait duration exceeds supported range".into())
        })?;
        let observe = async {
            loop {
                let state = self.state().await?;
                if state.is_terminal() {
                    return Ok(state);
                }
                if let RunState::Lost { reason } = state {
                    return Err(Error::State(format!("run status unknown: {reason}")));
                }
                sleep(Duration::from_millis(100)).await;
            }
        };
        timeout_at(deadline, observe)
            .await
            .map_err(|_| Error::Timeout("waiting for run; run was not canceled".into()))?
    }
}
