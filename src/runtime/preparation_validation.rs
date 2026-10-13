use super::command_factory::{AUTH_VARIABLES, PRIVATE_VARIABLES};
use super::credential_policy::launch_requirements;
use super::*;
use crate::{Error, Result};
use semver::{Version, VersionReq};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub(super) fn fingerprint(request: &PreparationRequest) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(request)?)
    ))
}

pub(super) fn safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && value != "."
        && value != ".."
}

pub(super) fn variable_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

pub(super) fn clean_absolute_path(path: &str) -> bool {
    path.starts_with('/')
        && path != "/"
        && !path.contains('\0')
        && !path.contains('\n')
        && !path.split('/').any(|part| part == ".." || part == ".")
}

pub(super) fn valid_recipe_command(command: &RecipeCommand) -> bool {
    !command.program.is_empty()
        && !command.program.contains('\0')
        && command.timeout_seconds > 0
        && command.args.iter().all(|arg| !arg.contains('\0'))
}

pub(super) fn validate(request: &PreparationRequest) -> Result<()> {
    if !safe_identifier(&request.id)
        || request.target_id.is_empty()
        || !safe_identifier(&request.recipe.id)
        || request.recipe.revision == 0
    {
        return Err(Error::InvalidInput(
            "preparation and recipe identifiers/revision must be valid".into(),
        ));
    }
    if !clean_absolute_path(&request.owned_root) || !clean_absolute_path(&request.working_directory)
    {
        return Err(Error::InvalidInput(
            "runtime and working directories must be normalized absolute paths".into(),
        ));
    }
    if request.owned_root == request.working_directory
        || request.owned_root.starts_with(&format!(
            "{}/",
            request.working_directory.trim_end_matches('/')
        ))
    {
        return Err(Error::InvalidInput("private runtime directory must be outside the project root and excluded from file sync".into()));
    }
    let version = Version::parse(&request.recipe.version).map_err(|_| {
        Error::InvalidInput("runtime installation requires an exact semver version".into())
    })?;
    let compatible = VersionReq::parse(&request.recipe.compatible_versions)
        .map_err(|_| Error::InvalidInput("invalid compatible version range".into()))?;
    if !compatible.matches(&version) {
        return Err(Error::InvalidInput(
            "pinned version must satisfy the compatible version range".into(),
        ));
    }
    if request.recipe.program.is_empty()
        || request.recipe.program.contains('\0')
        || request
            .recipe
            .version_args
            .iter()
            .any(|arg| arg.contains('\0'))
    {
        return Err(Error::InvalidInput("invalid runtime probe command".into()));
    }
    if let Installation::Npm { package } = &request.recipe.installation {
        if package.is_empty()
            || package.starts_with('-')
            || package.contains(char::is_whitespace)
            || package.contains(':')
            || package.contains('\\')
            || package.contains("..")
            || package.trim_start_matches('@').contains('@')
            || request.recipe.program.contains('/')
        {
            return Err(Error::InvalidInput(
                "npm installation requires a package name and bare program name".into(),
            ));
        }
    }
    if let Installation::Command {
        source,
        command,
        relative_program,
    } = &request.recipe.installation
    {
        if source.is_empty()
            || !valid_recipe_command(command)
            || relative_program.is_empty()
            || relative_program.starts_with('/')
            || relative_program
                .split('/')
                .any(|part| part == ".." || part == ".")
            || relative_program.contains('\0')
        {
            return Err(Error::InvalidInput(
                "invalid versioned installation command or relative program path".into(),
            ));
        }
    }
    let mut steps = BTreeSet::new();
    for step in &request.recipe.setup {
        if !safe_identifier(&step.id)
            || !steps.insert(&step.id)
            || !valid_recipe_command(&step.probe)
            || !valid_recipe_command(&step.apply)
        {
            return Err(Error::InvalidInput(
                "setup steps require unique ids and bounded valid commands".into(),
            ));
        }
    }
    let subscription = matches!(
        request.authentication,
        Authentication::Subscription | Authentication::CodexAuthCache { .. }
    );
    if subscription
        && !matches!(
            request.recipe.runtime,
            RuntimeKind::Codex
                | RuntimeKind::ClaudeCode
                | RuntimeKind::OpenCode {
                    provider: ModelProvider::OpenAi
                }
        )
    {
        return Err(Error::Unsupported("this runtime/provider requires explicit API or inference-provider credentials; Claude subscription tokens cannot be reused in OpenCode or an Agent SDK integration".into()));
    }
    if matches!(
        request.authentication,
        Authentication::CodexAuthCache { .. }
    ) && request.recipe.runtime != RuntimeKind::Codex
    {
        return Err(Error::Unsupported(
            "auth-cache provisioning is supported only for Codex".into(),
        ));
    }
    if matches!(request.authentication, Authentication::None)
        && request.recipe.runtime != RuntimeKind::Generic
    {
        return Err(Error::InvalidInput(
            "select subscription or API authentication for this agent runtime".into(),
        ));
    }
    if let Authentication::ProviderCredentials { variables } = &request.authentication {
        if variables.is_empty() {
            return Err(Error::InvalidInput(
                "provider credentials cannot be empty".into(),
            ));
        }
    }
    let mut secret_variables = BTreeSet::new();
    for requirement in launch_requirements(request)? {
        if !variable_name(&requirement.variable)
            || requirement.credential.0.is_empty()
            || !secret_variables.insert(requirement.variable.clone())
            || requirement.variable == "CLAUDE_CODE_OAUTH_TOKEN"
            || PRIVATE_VARIABLES.contains(&requirement.variable.as_str())
            || (subscription && AUTH_VARIABLES.contains(&requirement.variable.as_str()))
        {
            return Err(Error::InvalidInput(
                "invalid, duplicate, or incompatible runtime secret requirement".into(),
            ));
        }
    }
    for requirement in request.recipe.setup.iter().flat_map(|step| &step.secrets) {
        if !variable_name(&requirement.variable)
            || requirement.credential.0.is_empty()
            || requirement.variable == "CLAUDE_CODE_OAUTH_TOKEN"
        {
            return Err(Error::InvalidInput(
                "invalid setup secret requirement".into(),
            ));
        }
    }
    for (key, value) in &request.environment.variables {
        if !variable_name(key)
            || value.contains('\0')
            || PRIVATE_VARIABLES.contains(&key.as_str())
            || secret_variables.contains(key)
            || (AUTH_VARIABLES.contains(&key.as_str())
                && !(key.starts_with("CLAUDE_CODE_USE_") && !subscription))
        {
            return Err(Error::InvalidInput("plain environment contains a reserved credential/routing variable or invalid value".into()));
        }
    }
    Ok(())
}
