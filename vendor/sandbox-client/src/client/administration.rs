//! Administration and resource operations using the shared HTTP transport.
use super::*;
use crate::service::SANDBOX_PATH;

impl Api {
    /// Add a repository to an environment.
    ///
    /// `POST /environments/{environmentId}/repos`.
    pub async fn add_environment_repo(
        &self,
        environment_id: &str,
        request: AddEnvironmentRepoRequest,
    ) -> Result<EnvironmentItemChangeResponse> {
        let api_path = format!("/environments/{}/repos", segment(environment_id)?);
        self.request("add_environment_repo", Method::POST, &api_path)
            .json(Some(&request))
            .send()
            .await
    }

    /// Read usage and resource totals for an API key.
    ///
    /// `GET /api-keys/{apiKeyId}/usage`.
    pub async fn api_key_usage(&self, api_key_id: &str) -> Result<ApiKeyUsageResponse> {
        let api_path = format!("/api-keys/{}/usage", segment(api_key_id)?);
        self.request("api_key_usage", Method::GET, &api_path)
            .send()
            .await
    }

    /// List API keys and their usage summaries.
    ///
    /// `GET /api-keys`.
    pub async fn api_keys(&self) -> Result<ApiKeysResponse> {
        self.request("api_keys", Method::GET, "/api-keys")
            .send()
            .await
    }

    /// Download an artifact from a Sandbox as bytes.
    ///
    /// `GET /sandboxes/{sandboxId}/artifacts`.
    pub async fn artifact(&self, sandbox_id: &str, path: &str) -> Result<Vec<u8>> {
        let api_path = format!(
            "{SANDBOX_PATH}/{}/artifacts",
            validate_sandbox_id(sandbox_id)?
        );
        self.request("artifact", Method::GET, &api_path)
            .query(Some(&[("path", path)]))
            .bytes()
            .await
    }

    /// Create a webhook and return its signing secret.
    ///
    /// `POST /webhooks`.
    pub async fn create_webhook(
        &self,
        request: WebhookCreateRequest,
    ) -> Result<WebhookSecretResponse> {
        self.request("create_webhook", Method::POST, "/webhooks")
            .json(Some(&request))
            .send()
            .await
    }

    /// Remove a repository from an environment.
    ///
    /// `DELETE /environments/{environmentId}/repos/{repositoryId}`.
    pub async fn delete_environment_repo(
        &self,
        environment_id: &str,
        repository_id: &str,
    ) -> Result<EnvironmentItemChangeResponse> {
        let api_path = format!(
            "/environments/{}/repos/{}",
            segment(environment_id)?,
            segment(repository_id)?
        );
        self.request("delete_environment_repo", Method::DELETE, &api_path)
            .send()
            .await
    }

    /// Remove a secret file from an environment.
    ///
    /// `DELETE /environments/{environmentId}/secret-files`.
    pub async fn delete_environment_secret_file(
        &self,
        environment_id: &str,
        path: &str,
    ) -> Result<EnvironmentItemChangeResponse> {
        let api_path = format!("/environments/{}/secret-files", segment(environment_id)?);
        self.request("delete_environment_secret_file", Method::DELETE, &api_path)
            .query(Some(&[("path", path)]))
            .send()
            .await
    }

    /// Remove an environment variable.
    ///
    /// `DELETE /environments/{environmentId}/vars/{key}`.
    pub async fn delete_environment_var(
        &self,
        environment_id: &str,
        key: &str,
    ) -> Result<EnvironmentItemChangeResponse> {
        let api_path = format!(
            "/environments/{}/vars/{}",
            segment(environment_id)?,
            segment(key)?
        );
        self.request("delete_environment_var", Method::DELETE, &api_path)
            .send()
            .await
    }

    /// Delete a named snapshot.
    ///
    /// `DELETE /named-snapshots/{name}`.
    pub async fn delete_named_snapshot(&self, name: &str) -> Result<NamedSnapshotDeletedResponse> {
        let api_path = format!("/named-snapshots/{}", segment(name)?);
        self.request("delete_named_snapshot", Method::DELETE, &api_path)
            .send()
            .await
    }

    /// Delete a webhook.
    ///
    /// `DELETE /webhooks/{webhookId}`.
    pub async fn delete_webhook(&self, webhook_id: &str) -> Result<WebhookDeleteResponse> {
        let api_path = format!("/webhooks/{}", segment(webhook_id)?);
        self.request("delete_webhook", Method::DELETE, &api_path)
            .send()
            .await
    }

    /// Create an independent Sandbox from an existing Sandbox.
    ///
    /// `POST /sandboxes/{sandboxId}/fork`.
    pub async fn fork(
        &self,
        sandbox_id: &str,
        request: Option<ForkRequest>,
        idempotency_key: Option<&str>,
    ) -> Result<SandboxActionResponse> {
        let api_path = format!("{SANDBOX_PATH}/{}/fork", validate_sandbox_id(sandbox_id)?);
        self.request("fork", Method::POST, &api_path)
            .json(request.as_ref())
            .header("Idempotency-Key", idempotency_key)
            .send()
            .await
    }

    /// Read the account data-retention policy.
    ///
    /// `GET /account/data-retention`.
    pub async fn get_data_retention(&self) -> Result<DataRetentionPolicyResponse> {
        self.request("get_data_retention", Method::GET, "/account/data-retention")
            .send()
            .await
    }

    /// Read a named snapshot.
    ///
    /// `GET /named-snapshots/{name}`.
    pub async fn get_named_snapshot(&self, name: &str) -> Result<NamedSnapshotInfoResponse> {
        let api_path = format!("/named-snapshots/{}", segment(name)?);
        self.request("get_named_snapshot", Method::GET, &api_path)
            .send()
            .await
    }

    /// Read webhook settings.
    ///
    /// `GET /webhooks/{webhookId}`.
    pub async fn get_webhook(&self, webhook_id: &str) -> Result<WebhookResponse> {
        let api_path = format!("/webhooks/{}", segment(webhook_id)?);
        self.request("get_webhook", Method::GET, &api_path)
            .send()
            .await
    }

    /// List named snapshots.
    ///
    /// `GET /named-snapshots`.
    pub async fn list_named_snapshots(&self) -> Result<NamedSnapshotListResponse> {
        self.request("list_named_snapshots", Method::GET, "/named-snapshots")
            .send()
            .await
    }

    /// List webhooks.
    ///
    /// `GET /webhooks`.
    pub async fn list_webhooks(&self) -> Result<WebhookListResponse> {
        self.request("list_webhooks", Method::GET, "/webhooks")
            .send()
            .await
    }

    /// List available repositories with optional filters and pagination.
    ///
    /// `GET /repos`.
    pub async fn repos(&self, query: Option<&ReposQuery>) -> Result<ReposResponse> {
        self.request("repos", Method::GET, "/repos")
            .query(query)
            .send()
            .await
    }

    /// Rotate and return a webhook signing secret.
    ///
    /// `POST /webhooks/{webhookId}/rotate`.
    pub async fn rotate_webhook_signing_secret(
        &self,
        webhook_id: &str,
    ) -> Result<WebhookSecretResponse> {
        let api_path = format!("/webhooks/{}/rotate", segment(webhook_id)?);
        self.request("rotate_webhook_signing_secret", Method::POST, &api_path)
            .send()
            .await
    }

    /// Save a named snapshot from a Sandbox.
    ///
    /// `POST /named-snapshots`.
    pub async fn save_named_snapshot(
        &self,
        request: NamedSnapshotSaveRequest,
    ) -> Result<NamedSnapshotSavingResponse> {
        self.request("save_named_snapshot", Method::POST, "/named-snapshots")
            .json(Some(&request))
            .send()
            .await
    }

    /// Read the configured secrets and secret files.
    ///
    /// `GET /secrets`.
    pub async fn secrets(&self) -> Result<SecretsResponse> {
        self.request("secrets", Method::GET, "/secrets")
            .send()
            .await
    }

    /// Select a repository for new Sandboxes.
    ///
    /// `POST /repos`.
    pub async fn select_repo(
        &self,
        request: RepoSelectionRequest,
    ) -> Result<RepoSelectionResponse> {
        self.request("select_repo", Method::POST, "/repos")
            .json(Some(&request))
            .send()
            .await
    }

    /// Create or replace an environment secret file.
    ///
    /// `PUT /environments/{environmentId}/secret-files`.
    pub async fn set_environment_secret_file(
        &self,
        environment_id: &str,
        request: SetEnvironmentSecretFileRequest,
    ) -> Result<EnvironmentItemChangeResponse> {
        let api_path = format!("/environments/{}/secret-files", segment(environment_id)?);
        self.request("set_environment_secret_file", Method::PUT, &api_path)
            .json(Some(&request))
            .send()
            .await
    }

    /// Create or replace an environment variable.
    ///
    /// `PUT /environments/{environmentId}/vars/{key}`.
    pub async fn set_environment_var(
        &self,
        environment_id: &str,
        key: &str,
        request: SetEnvironmentVarRequest,
    ) -> Result<EnvironmentItemChangeResponse> {
        let api_path = format!(
            "/environments/{}/vars/{}",
            segment(environment_id)?,
            segment(key)?
        );
        self.request("set_environment_var", Method::PUT, &api_path)
            .json(Some(&request))
            .send()
            .await
    }

    /// Update the account data-retention policy.
    ///
    /// `PATCH /account/data-retention`.
    /// Requires an interactive session token. Enabling deletion also requires
    /// the confirmation phrase in the request.
    pub async fn update_data_retention(
        &self,
        request: DataRetentionUpdateRequest,
    ) -> Result<DataRetentionPolicyResponse> {
        self.request(
            "update_data_retention",
            Method::PATCH,
            "/account/data-retention",
        )
        .json(Some(&request))
        .send()
        .await
    }

    /// Replace the configured secrets and secret files.
    /// Include every value that should remain in the configuration.
    ///
    /// `POST /secrets`.
    pub async fn update_secrets(&self, request: SecretsUpdateRequest) -> Result<SecretsResponse> {
        self.request("update_secrets", Method::POST, "/secrets")
            .json(Some(&request))
            .send()
            .await
    }

    /// Update webhook settings.
    ///
    /// `PATCH /webhooks/{webhookId}`.
    pub async fn update_webhook(
        &self,
        webhook_id: &str,
        request: WebhookUpdateRequest,
    ) -> Result<WebhookResponse> {
        let api_path = format!("/webhooks/{}", segment(webhook_id)?);
        self.request("update_webhook", Method::PATCH, &api_path)
            .json(Some(&request))
            .send()
            .await
    }

    /// Upgrade an environment to a new version.
    ///
    /// `POST /environments/{environmentId}/upgrade`.
    pub async fn upgrade_environment(
        &self,
        environment_id: &str,
        request: Option<UpgradeSandboxEnvironmentRequest>,
    ) -> Result<UpgradeSandboxEnvironmentResponse> {
        let api_path = format!("/environments/{}/upgrade", segment(environment_id)?);
        self.request("upgrade_environment", Method::POST, &api_path)
            .json(request.as_ref())
            .send()
            .await
    }
}
