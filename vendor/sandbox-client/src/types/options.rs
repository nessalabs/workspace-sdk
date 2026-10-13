use super::*;

/// Per-call query and organization header, independent of the configured scope.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitsOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(skip)]
    pub x_boat_org: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct CreateOptions {
    pub org: Option<String>,
    pub x_boat_org: Option<String>,
    pub idempotency_key: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DesktopQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vnc: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ReposQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,
}

/// Select one conversation when interrupting a sandbox; omit for the default scope.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct InterruptQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation: Option<String>,
}
