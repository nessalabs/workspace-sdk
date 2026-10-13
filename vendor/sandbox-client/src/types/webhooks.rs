//! Request and response types for webhooks.
use super::*;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookCreateRequest {
    #[serde(
        default,
        deserialize_with = "deserialize_optional_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub name: Option<Option<String>>,
    pub url: String,
    pub events: std::collections::BTreeSet<String>,
}
impl fmt::Debug for WebhookCreateRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebhookCreateRequest")
            .field("url_len", &self.url.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookDeleteResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: String,
    pub id: String,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for WebhookDeleteResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebhookDeleteResponse")
            .field("ok", &self.ok)
            .field("type__len", &self.type_.len())
            .field("id", &self.id)
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookListResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: String,
    pub webhooks: Vec<Webhook>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for WebhookListResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebhookListResponse")
            .field("ok", &self.ok)
            .field("type__len", &self.type_.len())
            .field("webhooks_count", &self.webhooks.len())
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Webhook {
    pub id: String,
    pub name: Option<String>,
    pub url: String,
    pub events: std::collections::BTreeSet<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for Webhook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Webhook")
            .field("id", &self.id)
            .field("url_len", &self.url.len())
            .field("created_at_len", &self.created_at.len())
            .field("updated_at_len", &self.updated_at.len())
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: String,
    pub webhook: Webhook,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for WebhookResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebhookResponse")
            .field("ok", &self.ok)
            .field("type__len", &self.type_.len())
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookSecretResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: String,
    pub webhook: Webhook,
    pub secret: String,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for WebhookSecretResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebhookSecretResponse")
            .field("ok", &self.ok)
            .field("type__len", &self.type_.len())
            .field("secret_len", &self.secret.len())
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WebhookUpdateRequest {
    #[serde(
        default,
        deserialize_with = "deserialize_optional_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub name: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<std::collections::BTreeSet<String>>,
}
impl fmt::Debug for WebhookUpdateRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebhookUpdateRequest")
            .finish_non_exhaustive()
    }
}
