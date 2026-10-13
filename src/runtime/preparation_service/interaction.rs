use super::PreparationService;
use crate::runtime::{
    PreparationRequest, RuntimeInputOutcome, RuntimeProcessRef, RuntimeProcessState,
    RuntimeTerminalSize,
};
use crate::{Error, OutputCursor, OutputPage, Result, SecretInput};
use std::time::Duration;
use tokio::time::timeout;

const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_INPUT_BYTES: usize = 65_536;
const MAX_OUTPUT_BYTES: u32 = 131_072;

impl PreparationService {
    async fn validate_process(
        &self,
        request: &PreparationRequest,
        process: &RuntimeProcessRef,
    ) -> Result<()> {
        let receipt = self.existing_receipt(request).await?;
        if receipt.authentication_process.as_ref() != Some(process) {
            return Err(Error::Conflict(
                "native login reference is not the current preparation attempt".into(),
            ));
        }
        if process.host_scope() != receipt.host_scope {
            return Err(Error::Conflict(
                "native login reference belongs to another host scope".into(),
            ));
        }
        Ok(())
    }

    async fn require_running(&self, process: &RuntimeProcessRef) -> Result<()> {
        match timeout(CONTROL_TIMEOUT, self.host.inspect_process(process))
            .await
            .map_err(|_| Error::Timeout("native login status check timed out".into()))??
        {
            RuntimeProcessState::Running => Ok(()),
            RuntimeProcessState::Exited { .. } => {
                Err(Error::Conflict("native login has already exited".into()))
            }
            RuntimeProcessState::Lost => Err(Error::State(
                "native login is lost; restore host evidence before controlling or replacing it"
                    .into(),
            )),
        }
    }

    /// Observe the exact saved login attempt. Lost state never resets its identity.
    /// Restore/reconnect the original host and retry this observation to reconcile
    /// uncertainty. Only a confirmed exit permits a later authentication attempt.
    /// A busy preparation fails promptly; the host call has a five-second budget.
    pub async fn authentication_process_state(
        &self,
        request: &PreparationRequest,
        process: &RuntimeProcessRef,
    ) -> Result<RuntimeProcessState> {
        let _lease = self.acquire(request).await?;
        self.validate_process(request, process).await?;
        timeout(CONTROL_TIMEOUT, self.host.inspect_process(process))
            .await
            .map_err(|_| Error::Timeout("native login status check timed out".into()))?
    }

    /// Read retained native-login text for the current attempt. `limit` bounds
    /// combined raw bytes (2..=128 KiB). Lossy UTF-8 decoding can expand the
    /// returned text to three times that size and replace split characters.
    /// Cursors count raw bytes. Output may contain echoed input or
    /// one-time codes; it is returned to the caller but never saved in receipts.
    /// The host call has a five-second budget. PTY screen state is a separate API.
    pub async fn read_authentication_output(
        &self,
        request: &PreparationRequest,
        process: &RuntimeProcessRef,
        cursor: OutputCursor,
        limit: u32,
    ) -> Result<OutputPage> {
        if !(2..=MAX_OUTPUT_BYTES).contains(&limit) {
            return Err(Error::InvalidInput(
                "native login output limit must be 2..=128 KiB".into(),
            ));
        }
        let _lease = self.acquire(request).await?;
        self.validate_process(request, process).await?;
        let page = timeout(
            CONTROL_TIMEOUT,
            self.host
                .read_process_output(process, cursor.clone(), limit),
        )
        .await
        .map_err(|_| Error::Timeout("native login output read timed out".into()))??;
        let raw_advance = page
            .next
            .stdout
            .saturating_sub(cursor.stdout)
            .saturating_add(page.next.stderr.saturating_sub(cursor.stderr));
        if page.stdout.len().saturating_add(page.stderr.len()) > 3 * limit as usize
            || page
                .stdout
                .chars()
                .count()
                .saturating_add(page.stderr.chars().count())
                > limit as usize
            || (!page.gap && raw_advance > u64::from(limit))
            || page.next.stdout < cursor.stdout
            || page.next.stderr < cursor.stderr
        {
            return Err(Error::State(
                "native login host returned an oversized page or regressed cursor".into(),
            ));
        }
        Ok(page)
    }

    /// Submit 1..=64 KiB of transient bytes to the current live login attempt.
    /// Bytes are neither persisted nor automatically replayed. Terminal echo can
    /// expose them through output. Status and submission each have a five-second
    /// budget; a timed-out submission returns Unknown because delivery may have
    /// happened. A short Accepted result identifies only the accepted prefix.
    pub async fn write_authentication_input(
        &self,
        request: &PreparationRequest,
        process: &RuntimeProcessRef,
        input: &SecretInput,
    ) -> Result<RuntimeInputOutcome> {
        let length = input.expose().len();
        if length == 0 || length > MAX_INPUT_BYTES {
            return Err(Error::InvalidInput(
                "native login input must contain 1..=64 KiB".into(),
            ));
        }
        let _lease = self.acquire(request).await?;
        self.validate_process(request, process).await?;
        self.require_running(process).await?;
        match timeout(
            CONTROL_TIMEOUT,
            self.host.write_process_input(process, input),
        )
        .await
        {
            Ok(Ok(RuntimeInputOutcome::Accepted { bytes })) if bytes > length => {
                Ok(RuntimeInputOutcome::Unknown)
            }
            Ok(result) => result,
            Err(_) => Ok(RuntimeInputOutcome::Unknown),
        }
    }

    /// Resize the current live native login. Status and resize each have a
    /// five-second budget. A timeout leaves resize success unconfirmed; it never
    /// replaces the session. Hosts without a PTY return Unsupported.
    pub async fn resize_authentication(
        &self,
        request: &PreparationRequest,
        process: &RuntimeProcessRef,
        size: RuntimeTerminalSize,
    ) -> Result<()> {
        let _lease = self.acquire(request).await?;
        self.validate_process(request, process).await?;
        self.require_running(process).await?;
        timeout(CONTROL_TIMEOUT, self.host.resize_process(process, size))
            .await
            .map_err(|_| Error::Timeout("native login resize was not confirmed".into()))?
    }

    /// Cancel the exact current login attempt and retain its receipt, even if the
    /// response is lost. The host call has a five-second budget. After error or
    /// timeout inspect the same process; do not assume it stopped. Authentication
    /// success remains a separate native status check.
    pub async fn cancel_authentication(
        &self,
        request: &PreparationRequest,
        process: &RuntimeProcessRef,
    ) -> Result<RuntimeProcessState> {
        let _lease = self.acquire(request).await?;
        self.validate_process(request, process).await?;
        timeout(CONTROL_TIMEOUT, self.host.cancel_process(process))
            .await
            .map_err(|_| {
                Error::Timeout(
                    "native login cancellation was not confirmed; inspect the same attempt".into(),
                )
            })?
    }
}
