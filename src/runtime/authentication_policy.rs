use super::command_factory::runtime_command;
use super::*;
use crate::{Error, Result};
use serde::Deserialize;

/// The known fields used from native Claude Code status output. Other CLI fields
/// are ignored because they do not establish the selected subscription method.
#[derive(Deserialize)]
struct NativeClaudeStatus {
    #[serde(rename = "loggedIn", default)]
    logged_in: bool,
    #[serde(rename = "authMethod")]
    authentication_method: Option<String>,
}

pub(super) fn authentication_action(
    request: &PreparationRequest,
    receipt: &PreparationReceipt,
) -> AuthenticationAction {
    let (kind, description) = match (&request.authentication, &request.recipe.runtime) {
        (Authentication::CodexAuthCache { .. }, RuntimeKind::Codex) if !receipt.codex_cache_provisioned => (AuthenticationActionKind::ProvisionCodexCache, "Provision the explicitly supplied file-backed Codex auth.json into this target's private runtime directory"),
        (_, RuntimeKind::Codex) => (AuthenticationActionKind::DeviceLogin, "Start Codex device login on this target; complete its code in your browser"),
        (_, RuntimeKind::OpenCode { .. }) => (AuthenticationActionKind::DeviceLogin, "Start OpenCode's native ChatGPT headless login; complete its code in your browser"),
        _ => (AuthenticationActionKind::NativeTerminalRequired, "Run unmodified Claude Code's native login in the target terminal and complete your own subscription sign-in"),
    };
    AuthenticationAction {
        preparation_id: request.id.clone(),
        kind,
        description: description.into(),
    }
}

pub(super) fn login_command(
    request: &PreparationRequest,
    receipt: &PreparationReceipt,
) -> Result<RuntimeCommand> {
    if !matches!(
        request.authentication,
        Authentication::Subscription | Authentication::CodexAuthCache { .. }
    ) {
        return Err(Error::Unsupported(
            "native login requires an explicitly selected subscription authentication method"
                .into(),
        ));
    }
    let args = match request.recipe.runtime {
        RuntimeKind::Codex => vec!["login", "--device-auth"],
        RuntimeKind::OpenCode {
            provider: ModelProvider::OpenAi,
        } => vec![
            "auth",
            "login",
            "--provider",
            "openai",
            "--method",
            "ChatGPT Pro/Plus (headless)",
        ],
        RuntimeKind::ClaudeCode => vec!["auth", "login"],
        _ => {
            return Err(Error::Unsupported(
                "native subscription login is unavailable for this integration".into(),
            ))
        }
    };
    let program = receipt
        .installed_program
        .clone()
        .ok_or_else(|| Error::State("runtime installation has not completed".into()))?;
    Ok(runtime_command(
        request,
        program,
        args.into_iter().map(str::to_owned).collect(),
        900,
    ))
}

pub(super) fn native_subscription_present(kind: &RuntimeKind, output: &RuntimeOutput) -> bool {
    if output.exit_code != 0 {
        return false;
    }
    let text = format!("{}\n{}", output.stdout, output.stderr).to_lowercase();
    match kind {
        RuntimeKind::Codex => text.contains("logged in using chatgpt"),
        RuntimeKind::OpenCode {
            provider: ModelProvider::OpenAi,
        } => text
            .lines()
            .any(|line| line.contains("openai") && line.contains("oauth")),
        RuntimeKind::ClaudeCode => serde_json::from_str::<NativeClaudeStatus>(&output.stdout)
            .is_ok_and(|status| {
                status.logged_in && status.authentication_method.as_deref() == Some("claude.ai")
            }),
        _ => false,
    }
}
