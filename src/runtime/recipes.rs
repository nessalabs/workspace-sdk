use super::*;

fn npm_recipe(
    id: &str,
    kind: RuntimeKind,
    program: &str,
    package: &str,
    version: impl Into<String>,
) -> RuntimeRecipe {
    let version = version.into();
    RuntimeRecipe {
        id: id.into(),
        revision: 1,
        runtime: kind,
        program: program.into(),
        compatible_versions: format!("={version}"),
        version,
        version_args: vec!["--version".into()],
        installation: Installation::Npm {
            package: package.into(),
        },
        setup: Vec::new(),
    }
}

/// Construct a pinned recipe for the official `@openai/codex` npm package.
///
/// The initial compatibility range accepts exactly the supplied version, the version
/// probe is `--version`, and there are no additional setup steps. Construction has
/// no side effects; [`super::PreparationService`] validates the version and decides
/// whether reuse or a private installation is needed.
pub fn codex(version: impl Into<String>) -> RuntimeRecipe {
    npm_recipe(
        "codex",
        RuntimeKind::Codex,
        "codex",
        "@openai/codex",
        version,
    )
}

/// Construct a pinned `opencode-ai` recipe for the explicitly selected provider.
///
/// The provider selects authentication policy and private state scope. The initial
/// compatibility range is exact and no setup steps are added. This helper does not
/// install software, authenticate, launch a server, or call a model.
pub fn opencode(version: impl Into<String>, provider: ModelProvider) -> RuntimeRecipe {
    npm_recipe(
        "opencode",
        RuntimeKind::OpenCode { provider },
        "opencode",
        "opencode-ai",
        version,
    )
}

/// Construct a pinned recipe for the official, unmodified Claude Code npm package.
///
/// Native end-user authentication remains owned by the installed CLI; the caller
/// selects subscription or explicit API/provider credentials in the preparation
/// request. The initial compatibility range is exact, with a `--version` probe.
pub fn claude_code(version: impl Into<String>) -> RuntimeRecipe {
    npm_recipe(
        "claude-code",
        RuntimeKind::ClaudeCode,
        "claude",
        "@anthropic-ai/claude-code",
        version,
    )
}

/// Construct an existing-only recipe for an ordinary exact-version executable.
///
/// The initial recipe uses `--version`, exact semver compatibility, and no setup
/// steps. Callers may change its explicit installation policy or add repeatable
/// setup for Python packages, datasets, training libraries, or their own server.
/// Construction only builds intent; preparation validates and executes it.
pub fn generic(
    id: impl Into<String>,
    program: impl Into<String>,
    version: impl Into<String>,
) -> RuntimeRecipe {
    let version = version.into();
    RuntimeRecipe {
        id: id.into(),
        revision: 1,
        runtime: RuntimeKind::Generic,
        program: program.into(),
        compatible_versions: format!("={version}"),
        version,
        version_args: vec!["--version".into()],
        installation: Installation::ExistingOnly,
        setup: Vec::new(),
    }
}
