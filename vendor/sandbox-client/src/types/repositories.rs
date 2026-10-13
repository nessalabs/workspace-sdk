//! Request and response types for repositories.
use super::*;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReposResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: String,
    pub installations: Vec<RepositoryInstallation>,
    pub environment_id: String,
    pub selected_repositories: Vec<SelectedRepository>,
    pub page_info: Option<PageInfo>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for ReposResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReposResponse")
            .field("ok", &self.ok)
            .field("type__len", &self.type_.len())
            .field("installations_count", &self.installations.len())
            .field("environment_id_len", &self.environment_id.len())
            .field(
                "selected_repositories_count",
                &self.selected_repositories.len(),
            )
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryInstallation {
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub account_login: Option<String>,
    pub account_avatar_url: Option<String>,
    pub repositories: Option<Vec<Repository>>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for RepositoryInstallation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RepositoryInstallation")
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    pub id: Option<u64>,
    pub database_id: Option<String>,
    pub name: Option<String>,
    pub full_name: Option<String>,
    #[serde(rename = "private")]
    pub private: Option<bool>,
    pub permissions: Option<String>,
    pub pushed_at: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for Repository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Repository")
            .field("id", &self.id)
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoSelectionRequest {
    pub repository_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
}
impl fmt::Debug for RepoSelectionRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RepoSelectionRequest")
            .field("repository_id_len", &self.repository_id.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoSelectionResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: String,
    pub success: bool,
    pub environment_id: String,
    pub selected_repositories: Vec<SelectedRepository>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for RepoSelectionResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RepoSelectionResponse")
            .field("ok", &self.ok)
            .field("type__len", &self.type_.len())
            .field("success", &self.success)
            .field("environment_id_len", &self.environment_id.len())
            .field(
                "selected_repositories_count",
                &self.selected_repositories.len(),
            )
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretsResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: String,
    pub success: Option<bool>,
    pub environment_id: String,
    pub env_contents: String,
    pub secret_files: Vec<SecretFile>,
    pub pushed: Option<HashMap<String, serde_json::Value>>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for SecretsResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretsResponse")
            .field("ok", &self.ok)
            .field("type__len", &self.type_.len())
            .field("success", &self.success)
            .field("environment_id_len", &self.environment_id.len())
            .field("env_contents_len", &self.env_contents.len())
            .field("secret_files_count", &self.secret_files.len())
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SecretsUpdateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_contents: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_files: Option<Vec<SecretFile>>,
}
impl fmt::Debug for SecretsUpdateRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretsUpdateRequest")
            .finish_non_exhaustive()
    }
}
