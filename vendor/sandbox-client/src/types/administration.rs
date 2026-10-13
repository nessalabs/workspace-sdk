//! Request and response types for administration.
use super::*;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyUsageResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: String,
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub key_last_four: String,
    pub sandbox_id: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub usage: ApiKeyRequestUsage,
    pub resources: ApiKeyResourceTotals,
    pub created_resources: Vec<ApiKeyCreatedResource>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for ApiKeyUsageResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiKeyUsageResponse")
            .field("ok", &self.ok)
            .field("type__len", &self.type_.len())
            .field("id", &self.id)
            .field("name_len", &self.name.len())
            .field("key_prefix_len", &self.key_prefix.len())
            .field("key_last_four_len", &self.key_last_four.len())
            .field("sandbox_id_len", &self.sandbox_id.as_ref().map(String::len))
            .field("created_at_len", &self.created_at.len())
            .field(
                "last_used_at_len",
                &self.last_used_at.as_ref().map(String::len),
            )
            .field("created_resources_count", &self.created_resources.len())
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyRequestUsage {
    pub requests: u64,
    pub window_days: u32,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for ApiKeyRequestUsage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiKeyRequestUsage")
            .field("requests", &self.requests)
            .field("window_days", &self.window_days)
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyCreatedResource {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub state: String,
    pub created_at: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for ApiKeyCreatedResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiKeyCreatedResource")
            .field("kind_len", &self.kind.len())
            .field("id", &self.id)
            .field("name_len", &self.name.len())
            .field("state_len", &self.state.len())
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyResourceTotals {
    pub total: u64,
    pub sandboxes: u64,
    pub agents: u64,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for ApiKeyResourceTotals {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiKeyResourceTotals")
            .field("total", &self.total)
            .field("sandboxes", &self.sandboxes)
            .field("agents", &self.agents)
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeysResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: String,
    pub api_keys: Vec<ApiKey>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for ApiKeysResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiKeysResponse")
            .field("ok", &self.ok)
            .field("type__len", &self.type_.len())
            .field("api_keys_count", &self.api_keys.len())
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKey {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub key_last_four: String,
    pub sandbox_id: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub usage: ApiKeyRequestUsage,
    pub resources: ApiKeyResourceTotals,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiKey")
            .field("id", &self.id)
            .field("name_len", &self.name.len())
            .field("key_prefix_len", &self.key_prefix.len())
            .field("key_last_four_len", &self.key_last_four.len())
            .field("created_at_len", &self.created_at.len())
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataRetentionPolicyResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    pub enabled_at: Option<String>,
    pub queued_sandboxes: Option<u64>,
    pub accepted_deletion_operations_irreversible: Option<bool>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for DataRetentionPolicyResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DataRetentionPolicyResponse")
            .field("ok", &self.ok)
            .field("type__len", &self.type_.len())
            .field("enabled", &self.enabled)
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataRetentionUpdateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmation: Option<String>,
    /// Boat v1 `snapshotsOff` field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshots_off: Option<bool>,
    /// Boat v1 `monitoringOff` field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitoring_off: Option<bool>,
}
impl fmt::Debug for DataRetentionUpdateRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DataRetentionUpdateRequest")
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}
