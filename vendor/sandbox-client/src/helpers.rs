//! Convenience functions for commands, UTF-8 files, and Sandbox cleanup.

use crate::client::Api;
use crate::error::Result;
use crate::types::{CommandRequest, CommandResponse, FileWriteRequest, FileWriteResponse};

/// Run a shell command with an optional working directory and a 30-second default timeout.
pub async fn exec_command(
    api: &Api,
    sandbox_id: &str,
    command: impl Into<String>,
    cwd: Option<&str>,
    timeout_seconds: Option<u32>,
) -> Result<CommandResponse> {
    let mut req = CommandRequest::new(command).with_timeout_seconds(timeout_seconds.unwrap_or(30));
    if let Some(cwd) = cwd {
        req = req.with_cwd(cwd);
    }
    api.command(sandbox_id, req).await
}

/// Read a file as UTF-8 text.
pub async fn read_text(api: &Api, sandbox_id: &str, path: impl Into<String>) -> Result<String> {
    let res = api.read_file(sandbox_id, path, Some("utf8")).await?;
    Ok(res.content.unwrap_or_default())
}

/// Write UTF-8 text to a file.
pub async fn write_text(
    api: &Api,
    sandbox_id: &str,
    path: impl Into<String>,
    content: impl Into<String>,
) -> Result<FileWriteResponse> {
    api.write_file(
        sandbox_id,
        FileWriteRequest {
            path: path.into(),
            content: content.into(),
            encoding: Some("utf8".into()),
        },
    )
    .await
}

/// Stop and archive a Sandbox, retaining its snapshots for later resume.
pub async fn stop_and_remove(
    api: &Api,
    sandbox_id: &str,
) -> Result<crate::types::SandboxActionResponse> {
    api.stop(sandbox_id, None).await
}

/// Stop normally, or permanently delete when `delete` is true.
pub async fn stop_and_remove_with(
    api: &Api,
    sandbox_id: &str,
    delete: bool,
) -> Result<crate::types::StopOrDeleteResponse> {
    use crate::types::StopOrDeleteResponse;
    if delete {
        api.delete_sandbox(sandbox_id)
            .await
            .map(|response| StopOrDeleteResponse::Deleting(std::boxed::Box::new(response)))
    } else {
        api.stop(sandbox_id, None)
            .await
            .map(|response| StopOrDeleteResponse::Stopped(std::boxed::Box::new(response)))
    }
}
