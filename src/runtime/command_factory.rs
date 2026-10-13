use super::runtime_paths::*;
use super::*;
use std::collections::BTreeMap;

pub(super) const AUTH_VARIABLES: &[&str] = &[
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AWS_PROFILE",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "AZURE_API_KEY",
    "OPENCODE_SERVER_PASSWORD",
];
pub(super) const PRIVATE_VARIABLES: &[&str] = &[
    "CODEX_HOME",
    "CLAUDE_CONFIG_DIR",
    "XDG_DATA_HOME",
    "XDG_CONFIG_HOME",
    "XDG_STATE_HOME",
    "XDG_CACHE_HOME",
];

pub(super) fn runtime_command(
    request: &PreparationRequest,
    program: String,
    args: Vec<String>,
    timeout_seconds: u64,
) -> RuntimeCommand {
    let mut environment = request.environment.variables.clone();
    let root = auth_root(request);
    match request.recipe.runtime {
        RuntimeKind::Codex => {
            environment.insert("CODEX_HOME".into(), format!("{root}/codex"));
        }
        RuntimeKind::ClaudeCode | RuntimeKind::ClaudeAgentSdk => {
            environment.insert("CLAUDE_CONFIG_DIR".into(), format!("{root}/claude"));
        }
        RuntimeKind::OpenCode { .. } => {
            for (variable, part) in [
                ("XDG_DATA_HOME", "data"),
                ("XDG_CONFIG_HOME", "config"),
                ("XDG_STATE_HOME", "state"),
                ("XDG_CACHE_HOME", "cache"),
            ] {
                environment.insert(variable.into(), format!("{root}/opencode/{part}"));
            }
        }
        RuntimeKind::Generic => {}
    }
    RuntimeCommand {
        program,
        args,
        working_directory: request.working_directory.clone(),
        environment,
        secret_environment: BTreeMap::new(),
        remove_environment: AUTH_VARIABLES
            .iter()
            .chain(PRIVATE_VARIABLES)
            .map(|key| (*key).into())
            .collect(),
        timeout_seconds,
    }
}

pub(super) fn recipe_command(
    request: &PreparationRequest,
    command: &RecipeCommand,
) -> RuntimeCommand {
    runtime_command(
        request,
        command.program.clone(),
        command
            .args
            .iter()
            .map(|arg| arg.replace("{prefix}", &prefix(request)))
            .collect(),
        command.timeout_seconds,
    )
}

pub(super) fn installation_command(request: &PreparationRequest) -> Option<RuntimeCommand> {
    match &request.recipe.installation {
        Installation::ExistingOnly => None,
        Installation::Command { command, .. } => Some(recipe_command(request, command)),
        Installation::Npm { package } => Some(runtime_command(
            request,
            "npm".into(),
            vec![
                "install".into(),
                "--prefix".into(),
                prefix(request),
                "--no-audit".into(),
                "--no-fund".into(),
                format!("{package}@{}", request.recipe.version),
            ],
            600,
        )),
    }
}
