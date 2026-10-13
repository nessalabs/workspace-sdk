use super::*;
use sha2::{Digest, Sha256};

pub(super) fn auth_root(request: &PreparationRequest) -> String {
    // Separate selected billing methods, providers, and credential references.
    // In particular an OpenCode OAuth cache must not override a later API choice.
    let scope = format!("{:?}:{:?}", request.recipe.runtime, request.authentication);
    format!(
        "{}/auth/{:x}",
        request.owned_root.trim_end_matches('/'),
        Sha256::digest(scope.as_bytes())
    )
}

pub(super) fn prefix(request: &PreparationRequest) -> String {
    format!(
        "{}/tools/{}/{}",
        request.owned_root.trim_end_matches('/'),
        request.recipe.id,
        request.recipe.version
    )
}

pub(super) fn owned_program(request: &PreparationRequest) -> String {
    match &request.recipe.installation {
        Installation::Npm { .. } => format!(
            "{}/node_modules/.bin/{}",
            prefix(request),
            request.recipe.program
        ),
        Installation::Command {
            relative_program, ..
        } => format!("{}/{}", prefix(request), relative_program),
        Installation::ExistingOnly => request.recipe.program.clone(),
    }
}

pub(super) fn private_directories(request: &PreparationRequest) -> Vec<String> {
    let root = auth_root(request);
    match request.recipe.runtime {
        RuntimeKind::Codex => vec![format!("{root}/codex")],
        RuntimeKind::ClaudeCode | RuntimeKind::ClaudeAgentSdk => vec![format!("{root}/claude")],
        RuntimeKind::OpenCode { .. } => ["data", "config", "state", "cache"]
            .into_iter()
            .map(|part| format!("{root}/opencode/{part}"))
            .collect(),
        RuntimeKind::Generic => Vec::new(),
    }
}
