use super::*;
use crate::{Error, Result};

pub(super) fn valid_secret(value: &SecretValue) -> bool {
    !value.is_empty() && !value.expose().contains('\0')
}

pub(super) fn input_reason(resolution: &CredentialResolution) -> InputReason {
    match resolution {
        CredentialResolution::Expired => InputReason::Expired,
        _ => InputReason::Missing,
    }
}

pub(super) fn required_input(
    requirement: &SecretRequirement,
    resolution: &CredentialResolution,
) -> RequiredInput {
    RequiredInput {
        credential: requirement.credential.clone(),
        description: requirement.description.clone(),
        reason: input_reason(resolution),
    }
}

pub(super) fn auth_requirements(request: &PreparationRequest) -> Result<Vec<SecretRequirement>> {
    let requirement = |variable: &str, credential: &CredentialRef| SecretRequirement {
        variable: variable.into(),
        credential: credential.clone(),
        description: format!("{variable} for the explicitly selected API billing method"),
    };
    Ok(match &request.authentication {
        Authentication::ApiKey { credential } => {
            let variable = match &request.recipe.runtime {
                RuntimeKind::Codex
                | RuntimeKind::OpenCode {
                    provider: ModelProvider::OpenAi,
                } => "OPENAI_API_KEY",
                RuntimeKind::ClaudeCode
                | RuntimeKind::ClaudeAgentSdk
                | RuntimeKind::OpenCode {
                    provider: ModelProvider::Anthropic,
                } => "ANTHROPIC_API_KEY",
                _ => return Err(Error::Unsupported(
                    "declare named provider credentials for a generic or custom-provider runtime"
                        .into(),
                )),
            };
            vec![requirement(variable, credential)]
        }
        Authentication::ProviderCredentials { variables } => variables
            .iter()
            .map(|(key, credential)| requirement(key, credential))
            .collect(),
        _ => Vec::new(),
    })
}

pub(super) fn launch_requirements(request: &PreparationRequest) -> Result<Vec<SecretRequirement>> {
    let mut requirements = request.environment.secrets.clone();
    requirements.extend(auth_requirements(request)?);
    Ok(requirements)
}

pub(super) fn requirements(
    request: &PreparationRequest,
    include_cache: bool,
) -> Result<Vec<SecretRequirement>> {
    let mut requirements = launch_requirements(request)?;
    requirements.extend(
        request
            .recipe
            .setup
            .iter()
            .flat_map(|step| step.secrets.clone()),
    );
    if include_cache {
        if let Authentication::CodexAuthCache { credential } = &request.authentication {
            requirements.push(SecretRequirement {
                variable: String::new(),
                credential: credential.clone(),
                description: "Explicit file-backed Codex auth.json".into(),
            });
        }
    }
    Ok(requirements)
}
