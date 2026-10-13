use super::command_factory::*;
use super::credential_policy::*;
use super::preparation_validation::*;
use super::runtime_paths::*;
use super::*;
use crate::{Error, Result};
use semver::{Version, VersionReq};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Application service: orchestrates domain rules over injected ports. No cloud
/// SDK, ambient credential discovery, OAuth implementation, or model call lives here.
pub struct PreparationService {
    host: Arc<dyn IRuntimeHost>,
    credentials: Arc<dyn ICredentialResolver>,
    store: Arc<dyn IPreparationStore>,
}

impl PreparationService {
    async fn acquire(&self, request: &PreparationRequest) -> Result<Box<dyn IPreparationLease>> {
        validate(request)?;
        self.host.validate_target(&request.target_id)?;
        self.store.try_acquire(&request.id).await
    }

    fn validate_receipt(
        &self,
        request: &PreparationRequest,
        receipt: &PreparationReceipt,
    ) -> Result<()> {
        if receipt.id != request.id
            || receipt.target_id != request.target_id
            || receipt.host_scope != self.host.scope_id()?
            || receipt.request_fingerprint != fingerprint(request)?
        {
            return Err(Error::Conflict(
                "preparation receipt belongs to a different request or host".into(),
            ));
        }
        Ok(())
    }
    pub fn new(
        host: Arc<dyn IRuntimeHost>,
        credentials: Arc<dyn ICredentialResolver>,
        store: Arc<dyn IPreparationStore>,
    ) -> Self {
        Self {
            host,
            credentials,
            store,
        }
    }

    pub async fn prepare(&self, request: &PreparationRequest) -> Result<PreparationOutcome> {
        let _lease = self.acquire(request).await?;
        let mut receipt = self.receipt(request).await?;
        let resources = self.host.inspect_resources().await?;
        request
            .environment
            .resources
            .validate_available(&resources)?;
        let requirements = requirements(request, !receipt.codex_cache_provisioned)?;
        let (resolved, inputs) = self.resolve(&requirements).await?;
        if !inputs.is_empty() {
            self.store.save(&receipt).await?;
            return Ok(PreparationOutcome::NeedsInput { receipt, inputs });
        }

        // Checks happen before installation or credential provisioning. The host
        // enforces private modes and rejects symlink traversal at this boundary.
        self.host
            .ensure_private_directory(&request.owned_root)
            .await?;
        receipt.stage = PreparationStage::Installing;
        self.store.save(&receipt).await?;
        let owned_program = owned_program(request);
        let mut found = None;
        let mut candidates = vec![owned_program.clone(), request.recipe.program.clone()];
        candidates.dedup();
        for program in candidates {
            if let Some(version) = self.probe_version(request, &program).await? {
                found = Some((program, version));
                break;
            }
        }
        let (program, version) = match found {
            Some(found) => found,
            None => {
                let command = match installation_command(request) {
                    Some(command) => command,
                    None => return self.failed(receipt, PreparationStage::Installing, "required compatible program is unavailable and no installer was selected", false).await,
                };
                let installed = self.host.exec(command).await;
                if !matches!(installed, Ok(ref output) if output.exit_code == 0) {
                    return self.failed(receipt, PreparationStage::Installing, "runtime installation failed; check installer availability and selected source", true).await;
                }
                match self.probe_version(request, &owned_program).await? {
                    Some(version) if version == request.recipe.version => (owned_program, version),
                    _ => {
                        return self
                            .failed(
                                receipt,
                                PreparationStage::Installing,
                                "installer did not provide the exact requested version",
                                true,
                            )
                            .await
                    }
                }
            }
        };
        receipt.installed_program = Some(program.clone());
        receipt.installed_version = Some(version);
        receipt.stage = PreparationStage::Configuring;
        self.store.save(&receipt).await?;

        // Recheck each step even after a successful receipt; receipts are not
        // evidence that a user has not subsequently modified the environment.
        for step in &request.recipe.setup {
            let probe = self.host.exec(recipe_command(request, &step.probe)).await?;
            if probe.exit_code != 0 {
                let mut apply = recipe_command(request, &step.apply);
                for secret in &step.secrets {
                    apply.secret_environment.insert(
                        secret.variable.clone(),
                        resolved[&secret.credential].clone(),
                    );
                }
                let applied = self.host.exec(apply).await;
                if !matches!(applied, Ok(ref output) if output.exit_code == 0) {
                    return self
                        .failed(
                            receipt,
                            PreparationStage::Configuring,
                            "a runtime setup step failed",
                            true,
                        )
                        .await;
                }
                let verified = self.host.exec(recipe_command(request, &step.probe)).await;
                if !matches!(verified, Ok(ref output) if output.exit_code == 0) {
                    return self
                        .failed(
                            receipt,
                            PreparationStage::Configuring,
                            "a runtime setup step failed its verification",
                            true,
                        )
                        .await;
                }
            }
            if !receipt.completed_steps.contains(&step.id) {
                receipt.completed_steps.push(step.id.clone());
            }
            self.store.save(&receipt).await?;
        }

        receipt.stage = PreparationStage::Authenticating;
        self.store.save(&receipt).await?;
        for directory in private_directories(request) {
            self.host.ensure_private_directory(&directory).await?;
        }
        if let (RuntimeKind::Codex, Authentication::ApiKey { credential }) =
            (&request.recipe.runtime, &request.authentication)
        {
            let key = &resolved[credential];
            let login = runtime_command(
                request,
                program.clone(),
                vec!["login".into(), "--with-api-key".into()],
                30,
            );
            let output = self.host.exec_with_secret_stdin(login, key).await;
            if !matches!(output, Ok(ref output) if output.exit_code == 0) {
                return self
                    .failed(
                        receipt,
                        PreparationStage::Authenticating,
                        "Codex native API-key login failed",
                        true,
                    )
                    .await;
            }
            let status = self
                .host
                .exec(runtime_command(
                    request,
                    program.clone(),
                    vec!["login".into(), "status".into()],
                    30,
                ))
                .await?;
            let text = format!("{} {}", status.stdout, status.stderr).to_lowercase();
            if status.exit_code != 0 || !text.contains("api key") {
                return self
                    .failed(
                        receipt,
                        PreparationStage::Authenticating,
                        "Codex did not confirm the explicitly selected API authentication method",
                        true,
                    )
                    .await;
            }
        }
        let authentication = self.auth_state(request, &receipt).await?;
        match authentication {
            AuthenticationState::NeedsAuthentication(action) => {
                Ok(PreparationOutcome::NeedsAuthentication { receipt, action })
            }
            AuthenticationState::NeedsInput(inputs) => {
                Ok(PreparationOutcome::NeedsInput { receipt, inputs })
            }
            _ => {
                let mut base = runtime_command(request, program, Vec::new(), 0);
                for requirement in launch_requirements(request)? {
                    // Resolve again near launch preparation. Rotation/expiry after
                    // installation must not leave stale material in a Ready result.
                    match self.credentials.resolve(&requirement.credential).await? {
                        CredentialResolution::Available(value) if valid_secret(&value) => {
                            base.secret_environment.insert(requirement.variable, value);
                        }
                        resolution => {
                            let inputs = vec![required_input(&requirement, &resolution)];
                            return Ok(PreparationOutcome::NeedsInput { receipt, inputs });
                        }
                    }
                }
                receipt.stage = PreparationStage::Ready;
                self.store.save(&receipt).await?;
                Ok(PreparationOutcome::Ready(PreparedRuntime {
                    receipt,
                    kind: request.recipe.runtime.clone(),
                    base,
                    authentication,
                }))
            }
        }
    }

    async fn resolve(
        &self,
        requirements: &[SecretRequirement],
    ) -> Result<(BTreeMap<CredentialRef, SecretValue>, Vec<RequiredInput>)> {
        let mut resolved = BTreeMap::new();
        let mut inputs = Vec::new();
        let mut checked = BTreeSet::new();
        for requirement in requirements {
            if !checked.insert(requirement.credential.clone()) {
                continue;
            }
            match self.credentials.resolve(&requirement.credential).await? {
                CredentialResolution::Available(value) if valid_secret(&value) => {
                    resolved.insert(requirement.credential.clone(), value);
                }
                resolution => inputs.push(required_input(requirement, &resolution)),
            }
        }
        Ok((resolved, inputs))
    }

    async fn probe_version(
        &self,
        request: &PreparationRequest,
        program: &str,
    ) -> Result<Option<String>> {
        let output = match self
            .host
            .exec(runtime_command(
                request,
                program.into(),
                request.recipe.version_args.clone(),
                30,
            ))
            .await
        {
            Ok(output) => output,
            // A transport error is not evidence that installation is missing.
            Err(error) => return Err(error),
        };
        if output.exit_code != 0 {
            return Ok(None);
        }
        let compatible = VersionReq::parse(&request.recipe.compatible_versions)
            .map_err(|_| Error::InvalidInput("invalid compatible version range".into()))?;
        let version = format!("{} {}", output.stdout, output.stderr)
            .split_whitespace()
            .find_map(|part| {
                Version::parse(
                    part.trim_matches(|c: char| {
                        !(c.is_ascii_alphanumeric() || c == '.' || c == '+' || c == '-')
                    })
                    .trim_start_matches('v'),
                )
                .ok()
            });
        Ok(version
            .filter(|version| compatible.matches(version))
            .map(|version| version.to_string()))
    }

    async fn receipt(&self, request: &PreparationRequest) -> Result<PreparationReceipt> {
        let fingerprint = fingerprint(request)?;
        if let Some(receipt) = self.store.load(&request.id).await? {
            self.validate_receipt(request, &receipt)?;
            return Ok(receipt);
        }
        Ok(PreparationReceipt {
            id: request.id.clone(),
            target_id: request.target_id.clone(),
            host_scope: self.host.scope_id()?,
            request_fingerprint: fingerprint,
            recipe_id: request.recipe.id.clone(),
            recipe_revision: request.recipe.revision,
            stage: PreparationStage::Validating,
            installed_program: None,
            installed_version: None,
            completed_steps: Vec::new(),
            authentication_process: None,
            authentication_attempt: 0,
            codex_cache_provisioned: false,
        })
    }

    async fn existing_receipt(&self, request: &PreparationRequest) -> Result<PreparationReceipt> {
        let receipt = self
            .store
            .load(&request.id)
            .await?
            .ok_or_else(|| Error::State("prepare the runtime before authenticating".into()))?;
        self.validate_receipt(request, &receipt)?;
        if receipt.installed_program.is_none() {
            return Err(Error::State(
                "runtime installation has not completed".into(),
            ));
        }
        Ok(receipt)
    }

    async fn failed(
        &self,
        mut receipt: PreparationReceipt,
        stage: PreparationStage,
        message: &str,
        retryable: bool,
    ) -> Result<PreparationOutcome> {
        receipt.stage = stage.clone();
        self.store.save(&receipt).await?;
        Ok(PreparationOutcome::SetupFailed {
            receipt,
            stage,
            message: message.into(),
            retryable,
        })
    }
}

#[cfg(test)]
#[path = "preparation_service/tests.rs"]
mod tests;

#[path = "preparation_service/authentication.rs"]
mod authentication;
#[path = "preparation_service/interaction.rs"]
mod interaction;
