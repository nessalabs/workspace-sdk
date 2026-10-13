use super::PreparationService;
use crate::runtime::authentication_policy::*;
use crate::runtime::command_factory::runtime_command;
use crate::runtime::credential_policy::*;
use crate::runtime::runtime_paths::auth_root;
use crate::runtime::*;
use crate::{Error, Result};
use serde::de::IgnoredAny;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

impl PreparationService {
    /// Read-only native status check; it never dumps an auth file or invokes a model.
    pub async fn check_authentication(
        &self,
        request: &PreparationRequest,
    ) -> Result<AuthenticationState> {
        let _lease = self.acquire(request).await?;
        let receipt = self.existing_receipt(request).await?;
        self.auth_state(request, &receipt).await
    }

    /// Returns the exact native login invocation for applications providing their
    /// own remote terminal. It contains no credential material.
    pub async fn login_command(&self, request: &PreparationRequest) -> Result<RuntimeCommand> {
        let _lease = self.acquire(request).await?;
        let receipt = self.existing_receipt(request).await?;
        login_command(request, &receipt)
    }

    /// Start/resume native user sign-in, or explicitly provision supported Codex
    /// auth.json once. Successful process exit is followed by an auth status check.
    pub async fn start_authentication(
        &self,
        request: &PreparationRequest,
        action: &AuthenticationAction,
    ) -> Result<AuthenticationSession> {
        let _lease = self.acquire(request).await?;
        let mut receipt = self.existing_receipt(request).await?;
        if action.preparation_id != request.id {
            return Err(Error::InvalidInput(
                "authentication action belongs to another preparation".into(),
            ));
        }
        let current = self.auth_state(request, &receipt).await?;
        match &current {
            AuthenticationState::NeedsAuthentication(expected) if expected.kind == action.kind => {}
            AuthenticationState::NeedsAuthentication(_) => {
                return Err(Error::Conflict(
                    "authentication action is no longer current".into(),
                ))
            }
            _ => {
                return Ok(AuthenticationSession {
                    preparation_id: request.id.clone(),
                    process: receipt.authentication_process,
                    state: current,
                })
            }
        }
        if matches!(action.kind, AuthenticationActionKind::ProvisionCodexCache) {
            let Authentication::CodexAuthCache { credential } = &request.authentication else {
                return Err(Error::InvalidInput(
                    "cache provisioning is supported only for explicitly selected Codex auth"
                        .into(),
                ));
            };
            match self.credentials.resolve(credential).await? {
                CredentialResolution::Available(value) if valid_secret(&value) => {
                    // Validate syntax without logging or storing the parsed secret.
                    // Native cache fields belong to the CLI. Validate the object
                    // envelope while ignoring rather than retaining token values.
                    serde_json::from_str::<BTreeMap<String, IgnoredAny>>(value.expose()).map_err(
                        |_| Error::InvalidInput("Codex auth cache must be a JSON object".into()),
                    )?;
                    self.host
                        .provision_private_file(
                            &format!("{}/codex/auth.json", auth_root(request)),
                            &value,
                        )
                        .await?;
                    receipt.codex_cache_provisioned = true;
                    self.store.save(&receipt).await?;
                }
                resolution => {
                    return Ok(AuthenticationSession {
                        preparation_id: request.id.clone(),
                        process: None,
                        state: AuthenticationState::NeedsInput(vec![RequiredInput {
                            credential: credential.clone(),
                            description: "Explicit file-backed Codex auth.json".into(),
                            reason: input_reason(&resolution),
                        }]),
                    })
                }
            }
        } else {
            if let Some(process) = &receipt.authentication_process {
                match self.host.inspect_process(process).await? {
                    RuntimeProcessState::Running => return Ok(AuthenticationSession { preparation_id: request.id.clone(), process: Some(process.clone()), state: current }),
                    RuntimeProcessState::Lost => return Err(Error::State("native login process is lost; reconcile it before creating another authentication attempt".into())),
                    RuntimeProcessState::Exited { .. } => {
                        receipt.authentication_process = None;
                        receipt.authentication_attempt = receipt.authentication_attempt.checked_add(1).ok_or_else(|| Error::State("too many authentication attempts".into()))?;
                    }
                }
            }
            let command = login_command(request, &receipt)?;
            self.store.save(&receipt).await?;
            let key = format!(
                "runtime-auth-{:x}-{}",
                Sha256::digest(
                    format!("{}:{}", receipt.host_scope, receipt.request_fingerprint).as_bytes()
                ),
                receipt.authentication_attempt
            );
            let process = self
                .host
                .start_interactive(
                    &key,
                    command,
                    matches!(
                        action.kind,
                        AuthenticationActionKind::NativeTerminalRequired
                    ),
                )
                .await?;
            if process.host_scope() != receipt.host_scope {
                return Err(Error::State(
                    "native login returned a process from another host scope".into(),
                ));
            }
            receipt.authentication_process = Some(process);
            self.store.save(&receipt).await?;
        }
        let state = self.auth_state(request, &receipt).await?;
        Ok(AuthenticationSession {
            preparation_id: request.id.clone(),
            process: receipt.authentication_process,
            state,
        })
    }

    pub(super) async fn auth_state(
        &self,
        request: &PreparationRequest,
        receipt: &PreparationReceipt,
    ) -> Result<AuthenticationState> {
        match &request.authentication {
            Authentication::None => Ok(AuthenticationState::NotRequired),
            Authentication::ApiKey { .. } | Authentication::ProviderCredentials { .. } => {
                let (_, inputs) = self.resolve(&auth_requirements(request)?).await?;
                Ok(if inputs.is_empty() {
                    AuthenticationState::CredentialsAvailable
                } else {
                    AuthenticationState::NeedsInput(inputs)
                })
            }
            Authentication::Subscription | Authentication::CodexAuthCache { .. } => {
                let program = receipt.installed_program.as_ref().ok_or_else(|| {
                    Error::State("prepare the runtime before authenticating".into())
                })?;
                let args: Vec<String> = match request.recipe.runtime {
                    RuntimeKind::Codex => vec!["login".into(), "status".into()],
                    RuntimeKind::OpenCode { .. } => vec!["auth".into(), "list".into()],
                    RuntimeKind::ClaudeCode => vec!["auth".into(), "status".into()],
                    _ => {
                        return Err(Error::Unsupported(
                            "subscription authentication is unavailable for this integration"
                                .into(),
                        ))
                    }
                };
                let output = self
                    .host
                    .exec(runtime_command(request, program.clone(), args, 30))
                    .await?;
                if native_subscription_present(&request.recipe.runtime, &output) {
                    return Ok(AuthenticationState::NativeSessionPresent);
                }
                let action = authentication_action(request, receipt);
                // Probe native options before advertising a device flow. No
                // speculative login process or billing fallback is attempted.
                if matches!(action.kind, AuthenticationActionKind::DeviceLogin) {
                    let (args, required): (Vec<String>, &[&str]) = match request.recipe.runtime {
                        RuntimeKind::Codex => {
                            (vec!["login".into(), "--help".into()], &["--device-auth"])
                        }
                        RuntimeKind::OpenCode { .. } => (
                            vec!["auth".into(), "login".into(), "--help".into()],
                            &["--provider", "--method"],
                        ),
                        _ => unreachable!(),
                    };
                    let features = self
                        .host
                        .exec(runtime_command(request, program.clone(), args, 30))
                        .await?;
                    let text = format!("{}{}", features.stdout, features.stderr);
                    if features.exit_code != 0
                        || required.iter().any(|option| !text.contains(option))
                    {
                        return Err(Error::Unsupported("installed runtime lacks the selected native login options; choose a supported pinned version or explicitly select API credentials".into()));
                    }
                }
                Ok(AuthenticationState::NeedsAuthentication(action))
            }
        }
    }
}
