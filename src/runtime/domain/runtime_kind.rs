//! Runtime and model-provider classification for preparation policy.
//!
//! These value objects select integration behavior and authentication rules. They
//! are separate from executable version selection, model IDs, and provider clients.

use serde::{Deserialize, Serialize};

/// The model provider selected for a provider-aware runtime such as OpenCode.
///
/// This affects authentication policy and private state separation. It is not a
/// model identifier or a provider client, and selecting it does not make a model call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelProvider {
    /// OpenAI; OpenCode supports its own native ChatGPT login or an explicit API key.
    OpenAi,
    /// Anthropic; OpenCode requires explicit API or inference-provider credentials
    /// rather than reuse of a Claude subscription token.
    Anthropic,
    /// Caller-named provider. Supply named credentials through
    /// [`crate::runtime::Authentication::ProviderCredentials`] when authentication is needed.
    Other(String),
}

/// The workload integration whose preparation and authentication rules apply.
///
/// A kind is part of a [`crate::runtime::RuntimeRecipe`] and scopes private native auth state.
/// It identifies integration behavior, while the recipe separately selects the
/// executable, version, installer, and setup steps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeKind {
    /// The native Codex CLI, supporting its own device login, explicit API-key login,
    /// and explicitly requested one-time provisioning of a file-backed auth cache.
    Codex,
    /// The OpenCode CLI with authentication behavior selected by its model provider.
    OpenCode {
        /// Model provider used to choose credentials or native subscription login.
        provider: ModelProvider,
    },
    /// The official, unmodified Claude Code CLI. End-user subscription authentication
    /// uses its native target-terminal login; explicit API/provider credentials are
    /// separate choices.
    ClaudeCode,
    /// A caller's Claude Agent SDK integration. It requires explicit API or
    /// inference-provider credentials; Claude subscription tokens are not accepted.
    ClaudeAgentSdk,
    /// An ordinary versioned program, such as a trainer, tool, or custom server. It
    /// can omit model authentication or declare named provider/workload credentials.
    Generic,
}
