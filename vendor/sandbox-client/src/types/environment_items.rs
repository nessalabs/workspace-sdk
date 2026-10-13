//! Request and response types for environment items.
use super::*;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddEnvironmentRepoRequest {
    pub repository_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
}
impl fmt::Debug for AddEnvironmentRepoRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AddEnvironmentRepoRequest")
            .field("repository_id_len", &self.repository_id.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentItemChangeResponse {
    pub success: bool,
    pub version_id: Option<String>,
    pub version_number: Option<u64>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for EnvironmentItemChangeResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnvironmentItemChangeResponse")
            .field("success", &self.success)
            .field("version_number", &self.version_number)
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetEnvironmentSecretFileRequest {
    pub path: String,
    pub contents: String,
}
impl fmt::Debug for SetEnvironmentSecretFileRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetEnvironmentSecretFileRequest")
            .field("path_len", &self.path.len())
            .field("contents_len", &self.contents.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetEnvironmentVarRequest {
    pub value: String,
}
impl fmt::Debug for SetEnvironmentVarRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetEnvironmentVarRequest")
            .field("value_len", &self.value.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeSandboxEnvironmentRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_ids: Option<Vec<String>>,
}
impl fmt::Debug for UpgradeSandboxEnvironmentRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpgradeSandboxEnvironmentRequest")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeSandboxEnvironmentResponse {
    pub success: bool,
    pub upgraded: u64,
    pub failed: u64,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for UpgradeSandboxEnvironmentResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpgradeSandboxEnvironmentResponse")
            .field("success", &self.success)
            .field("upgraded", &self.upgraded)
            .field("failed", &self.failed)
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}
