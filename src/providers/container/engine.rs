//! Docker/Podman CLI boundary and the specific inspection fields the SDK uses.
use super::ContainerConfig;
use crate::providers::{ICommandRunner, NativeCommand};
use crate::{Error, ExecResult, Result, WorkspaceId};
use serde::Deserialize;
use std::{collections::BTreeMap, time::Duration};
#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub(super) struct ContainerEnvironment {
    #[serde(default)]
    pub labels: Option<BTreeMap<String, String>>,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub(super) struct ContainerState {
    #[serde(default)]
    pub running: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct ContainerMount {
    #[serde(rename = "Type")]
    pub mount_type: String,
    pub destination: String,
    #[serde(default)]
    pub name: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct ContainerDetails {
    #[serde(default)]
    pub config: ContainerEnvironment,
    #[serde(default)]
    pub state: ContainerState,
    #[serde(default)]
    pub mounts: Vec<ContainerMount>,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct VolumeDetails {
    #[serde(default)]
    pub labels: Option<BTreeMap<String, String>>,
}
pub(super) fn name(value: &str) -> Result<()> {
    if value.is_empty()
        || value.starts_with('-')
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    {
        Err(Error::InvalidInput("invalid container identifier".into()))
    } else {
        Ok(())
    }
}
pub(super) async fn engine(
    config: &ContainerConfig,
    runner: &dyn ICommandRunner,
    args: Vec<String>,
) -> Result<ExecResult> {
    runner
        .run(NativeCommand {
            program: config.engine.clone(),
            args,
            stdin: vec![],
            timeout: Duration::from_secs(120),
        })
        .await
}
pub(super) fn succeeded(result: ExecResult, action: &str) -> Result<ExecResult> {
    if result.success() {
        Ok(result)
    } else {
        Err(Error::Provider {
            provider: "container".into(),
            message: format!("container engine {action} failed (raw output redacted)"),
        })
    }
}
pub(super) fn container_info(result: &ExecResult) -> Result<ContainerDetails> {
    serde_json::from_str(&result.stdout)
        .map_err(|_| Error::State("invalid container inspection response".into()))
}
pub(super) fn volume_info(result: &ExecResult) -> Result<VolumeDetails> {
    serde_json::from_str(&result.stdout)
        .map_err(|_| Error::State("invalid volume inspection response".into()))
}
pub(super) fn check_owner(value: &ContainerDetails, id: &WorkspaceId) -> Result<()> {
    if value
        .config
        .labels
        .as_ref()
        .and_then(|labels| labels.get("dev.workspace-sdk.workspace"))
        .map(String::as_str)
        != Some(id.as_str())
    {
        return Err(Error::Conflict(
            "existing container is not owned by this workspace".into(),
        ));
    }
    Ok(())
}
pub(super) fn check_volume_owner(value: &VolumeDetails, id: &WorkspaceId) -> Result<()> {
    if value
        .labels
        .as_ref()
        .and_then(|labels| labels.get("dev.workspace-sdk.workspace"))
        .map(String::as_str)
        != Some(id.as_str())
    {
        return Err(Error::Conflict(
            "existing named volume is not owned by this workspace".into(),
        ));
    }
    Ok(())
}
