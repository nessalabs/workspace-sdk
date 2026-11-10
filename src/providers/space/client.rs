use super::{
    configuration::{canonical_uuid, SpaceConfig},
    events,
};
use crate::{Error, Result, WorkspaceState};
use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::{header, Response, StatusCode, Url};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::json;
use std::time::{Duration, Instant};

const MAX_JSON: usize = 2 * 1024 * 1024;
pub(super) const MAX_REQUEST: usize = 16 * 1024 * 1024;
pub(super) fn remaining(deadline: Instant) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|value| !value.is_zero())
        .ok_or_else(|| {
            Error::Timeout(
                "SPACE RPC deadline expired; reconcile the same native run identity".into(),
            )
        })
}
pub(super) fn failure(message: &str) -> Error {
    Error::Provider {
        provider: "space".into(),
        message: message.into(),
    }
}
fn transport(error: reqwest::Error) -> Error {
    if error.is_timeout() {
        Error::Timeout("SPACE transport timed out; native execution may continue".into())
    } else {
        failure("SPACE transport failed; raw response and credentials redacted")
    }
}
async fn status(response: Response) -> Result<Response> {
    match response.status() {
        status if status.is_success() => Ok(response),
        StatusCode::NOT_FOUND => Err(Error::NotFound(
            "SPACE target or transport session is unavailable".into(),
        )),
        StatusCode::CONFLICT => Err(Error::Conflict("SPACE transport request conflicted".into())),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            Err(Error::CredentialRequired("SPACE access refused".into()))
        }
        _ => Err(failure(
            "SPACE HTTP operation failed; raw response redacted",
        )),
    }
}
async fn bounded_json<T: DeserializeOwned>(mut response: Response, deadline: Instant) -> Result<T> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_JSON as u64)
    {
        return Err(failure("SPACE JSON response exceeds bound"));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = tokio::time::timeout(remaining(deadline)?, response.chunk())
        .await
        .map_err(|_| Error::Timeout("SPACE JSON response deadline".into()))?
        .map_err(transport)?
    {
        if bytes
            .len()
            .checked_add(chunk.len())
            .is_none_or(|size| size > MAX_JSON)
        {
            return Err(failure("SPACE JSON response exceeds bound"));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| failure("SPACE JSON response is malformed; raw response redacted"))
}
impl SpaceConfig {
    fn api_url(&self, path: &str) -> Url {
        self.api
            .join(path)
            .expect("validated origin and fixed API path")
    }
    fn management(&self) -> &str {
        std::str::from_utf8(self.token.expose()).expect("validated ASCII credential")
    }
    pub(super) async fn sandbox_state(
        &self,
        sandbox: &str,
        deadline: Instant,
    ) -> Result<WorkspaceState> {
        #[derive(Deserialize)]
        struct Sandbox {
            id: String,
            state: String,
        }
        let response = self
            .client
            .get(self.api_url(&format!("/v1/sandboxes/{sandbox}")))
            .bearer_auth(self.management())
            .timeout(remaining(deadline)?)
            .send()
            .await
            .map_err(transport)?;
        let value: Sandbox = bounded_json(status(response).await?, deadline).await?;
        if value.id != sandbox {
            return Err(failure("SPACE returned another sandbox identity"));
        }
        Ok(match value.state.as_str() {
            "running" => WorkspaceState::Ready,
            "paused" | "suspended" => WorkspaceState::Stopped,
            "creating" | "starting" | "waking" => WorkspaceState::Provisioning,
            "deleted" => WorkspaceState::Deleted,
            _ => WorkspaceState::Unknown,
        })
    }
    /// A write request is issued exactly once. An error is ambiguous and callers
    /// must retain the prepared slot; files:write can create if its path vanished.
    pub(super) async fn upload(
        &self,
        sandbox: &str,
        path: &str,
        bytes: &[u8],
        deadline: Instant,
    ) -> Result<u64> {
        #[derive(Deserialize)]
        struct Written {
            bytes_written: u64,
        }
        if bytes.len() > MAX_REQUEST {
            return Err(Error::InvalidInput(
                "SPACE private request exceeds 16MiB".into(),
            ));
        }
        let body = json!({"path":path,"data_base64":STANDARD.encode(bytes),"append":true,"user":self.user});
        let response = self
            .client
            .post(self.api_url(&format!("/v1/sandboxes/{sandbox}/files:write")))
            .bearer_auth(self.management())
            .header("Idempotency-Key", uuid::Uuid::new_v4().to_string())
            .json(&body)
            .timeout(remaining(deadline)?)
            .send()
            .await
            .map_err(transport)?;
        let written: Written = bounded_json(status(response).await?, deadline).await?;
        Ok(written.bytes_written)
    }
    pub(super) async fn rpc(
        &self,
        sandbox: &str,
        root: &str,
        command: Vec<String>,
        deadline: Instant,
    ) -> Result<String> {
        if command.len() > 4096
            || command
                .iter()
                .map(String::len)
                .try_fold(0usize, |sum, size| sum.checked_add(size))
                .is_none_or(|size| size > 262144)
        {
            return Err(Error::InvalidInput(
                "SPACE native transport command exceeds bound".into(),
            ));
        }
        let body = json!({"command":command,"cwd":root,"env":{},"user":self.user,"stdin":false,"timeout":"30s"});
        let response = self
            .client
            .post(self.api_url(&format!("/v1/sandboxes/{sandbox}/commands")))
            .bearer_auth(self.management())
            .header("Idempotency-Key", uuid::Uuid::new_v4().to_string())
            .json(&body)
            .timeout(remaining(deadline)?)
            .send()
            .await
            .map_err(transport)?;
        let receipt: CommandReceipt = bounded_json(status(response).await?, deadline).await?;
        if receipt.sandbox_id != sandbox
            || !process_id(&receipt.session_id)
            || !process_id(&receipt.process_id)
            || !["pending", "attaching", "open", "reconnecting"].contains(&receipt.state.as_str())
            || receipt.attach.token_transport.is_empty()
            || receipt.attach.token_transport.len() > 8
            || receipt
                .attach
                .token_transport
                .iter()
                .any(|value| value.len() > 64)
            || !receipt
                .attach
                .token_transport
                .iter()
                .any(|value| value == "authorization_header")
            || receipt.attach.token.is_empty()
            || receipt.attach.token.len() > 16 * 1024
            || !receipt
                .attach
                .token
                .bytes()
                .all(|byte| matches!(byte, 0x21..=0x7e))
        {
            return Err(failure(
                "SPACE command receipt identity/authentication is invalid",
            ));
        }
        let url = self
            .data
            .join(&receipt.attach.events_url)
            .map_err(|_| failure("SPACE attachment URL is invalid"))?;
        if url.origin() != self.data.origin()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != format!("/v1/sessions/{}/events", receipt.session_id)
        {
            return Err(failure(
                "SPACE attachment endpoint does not match the configured data plane",
            ));
        }
        let response = self
            .client
            .get(url)
            .bearer_auth(&receipt.attach.token)
            .header(header::ACCEPT, "text/event-stream")
            .timeout(remaining(deadline)?)
            .send()
            .await
            .map_err(transport)?;
        let response = status(response).await?;
        if response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .map(str::trim)
            != Some("text/event-stream")
        {
            return Err(failure("SPACE attachment did not return an event stream"));
        }
        events::collect(response, &receipt.session_id, &receipt.process_id, deadline).await
    }
}
fn process_id(value: &str) -> bool {
    value.strip_prefix("proc_").is_some_and(canonical_uuid)
}
#[derive(Deserialize)]
struct CommandReceipt {
    session_id: String,
    process_id: String,
    sandbox_id: String,
    state: String,
    attach: Attachment,
}
#[derive(Deserialize)]
struct Attachment {
    events_url: String,
    token: String,
    token_transport: Vec<String>,
}
