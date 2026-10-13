use super::*;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretFile {
    pub path: String,
    pub contents: String,
}
impl fmt::Debug for SecretFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretFile")
            .field("path_len", &self.path.len())
            .field("contents_len", &self.contents.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedRepository {
    pub id: Option<u64>,
    pub database_id: Option<String>,
    pub name: Option<String>,
    pub full_name: Option<String>,
    #[serde(rename = "private")]
    pub private: Option<bool>,
    pub permissions: Option<String>,
    pub pushed_at: Option<String>,
    pub base_branch: Option<String>,
    pub setup_routine_id: Option<String>,
    pub setup_script: Option<String>,
    pub setup_blocking: Option<bool>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for SelectedRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SelectedRepository")
            .field("id", &self.id)
            .field(
                "database_id_len",
                &self.database_id.as_ref().map(String::len),
            )
            .field("name_len", &self.name.as_ref().map(String::len))
            .field("full_name_len", &self.full_name.as_ref().map(String::len))
            .field(
                "permissions_len",
                &self.permissions.as_ref().map(String::len),
            )
            .field("pushed_at_len", &self.pushed_at.as_ref().map(String::len))
            .field(
                "base_branch_len",
                &self.base_branch.as_ref().map(String::len),
            )
            .field(
                "setup_routine_id_len",
                &self.setup_routine_id.as_ref().map(String::len),
            )
            .field(
                "setup_script_len",
                &self.setup_script.as_ref().map(String::len),
            )
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxEnvironmentVersionSummary {
    pub id: String,
    pub version_number: u64,
    pub sandbox_count: u64,
    pub created_at: String,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for SandboxEnvironmentVersionSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SandboxEnvironmentVersionSummary")
            .field("id", &self.id)
            .field("version_number", &self.version_number)
            .field("sandbox_count", &self.sandbox_count)
            .field("created_at_len", &self.created_at.len())
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxEnvironment {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub latest_version_id: Option<String>,
    pub safe_for_third_parties: bool,
    pub pass_github: bool,
    pub pass_secrets: bool,
    pub pass_sandbox_credentials: bool,
    pub pass_agents_credentials: bool,
    pub env_contents: String,
    pub secret_files: Vec<SecretFile>,
    pub selected_repositories: Option<Vec<SelectedRepository>>,
    pub versions: Vec<SandboxEnvironmentVersionSummary>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for SandboxEnvironment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SandboxEnvironment")
            .field("id", &self.id)
            .field("name_len", &self.name.len())
            .field("is_default", &self.is_default)
            .field(
                "latest_version_id_len",
                &self.latest_version_id.as_ref().map(String::len),
            )
            .field("safe_for_third_parties", &self.safe_for_third_parties)
            .field("pass_github", &self.pass_github)
            .field("pass_secrets", &self.pass_secrets)
            .field("pass_sandbox_credentials", &self.pass_sandbox_credentials)
            .field("pass_agents_credentials", &self.pass_agents_credentials)
            .field("env_contents_len", &self.env_contents.len())
            .field("secret_files_count", &self.secret_files.len())
            .field(
                "selected_repositories_count",
                &self.selected_repositories.as_ref().map(Vec::len),
            )
            .field("versions_count", &self.versions.len())
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSandboxEnvironmentRequest {
    pub name: String,
}
impl fmt::Debug for CreateSandboxEnvironmentRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateSandboxEnvironmentRequest")
            .field("name_len", &self.name.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentRepository {
    pub repository_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup_script: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup_blocking: Option<bool>,
}
impl fmt::Debug for EnvironmentRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnvironmentRepository")
            .field("repository_id_len", &self.repository_id.len())
            .field(
                "base_branch_len",
                &self.base_branch.as_ref().map(String::len),
            )
            .field(
                "setup_script_len",
                &self.setup_script.as_ref().map(String::len),
            )
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSandboxEnvironmentRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_default: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safe_for_third_parties: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pass_github: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pass_secrets: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pass_sandbox_credentials: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pass_agents_credentials: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_contents: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_files: Option<Vec<SecretFile>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repositories: Option<Vec<EnvironmentRepository>>,
    /// Boat v1 `credentialProfileId` field.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub credential_profile_id: Option<Option<String>>,
    /// Boat v1 `proxyAiCredentials` field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_ai_credentials: Option<bool>,
}
impl fmt::Debug for UpdateSandboxEnvironmentRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateSandboxEnvironmentRequest")
            .field("name_len", &self.name.as_ref().map(String::len))
            .field("is_default", &self.is_default)
            .field("safe_for_third_parties", &self.safe_for_third_parties)
            .field("pass_github", &self.pass_github)
            .field("pass_secrets", &self.pass_secrets)
            .field("pass_sandbox_credentials", &self.pass_sandbox_credentials)
            .field("pass_agents_credentials", &self.pass_agents_credentials)
            .field(
                "env_contents_len",
                &self.env_contents.as_ref().map(String::len),
            )
            .field(
                "secret_files_count",
                &self.secret_files.as_ref().map(Vec::len),
            )
            .field(
                "repositories_count",
                &self.repositories.as_ref().map(Vec::len),
            )
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxEnvironmentListResponse {
    pub environments: Vec<SandboxEnvironment>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for SandboxEnvironmentListResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SandboxEnvironmentListResponse")
            .field("environments_count", &self.environments.len())
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxEnvironmentResponse {
    pub success: bool,
    pub environment: Option<SandboxEnvironment>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for SandboxEnvironmentResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SandboxEnvironmentResponse")
            .field("success", &self.success)
            .field("extra_count", &self.extra.len())
            .finish()
    }
}
