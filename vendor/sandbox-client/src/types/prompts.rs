use super::*;

/// Agent providers accepted by the prompt endpoint.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PromptProvider {
    Codex,
    ClaudeCode,
    Claude,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptRequest {
    pub provider: PromptProvider,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub model: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub reasoning_effort: Option<Option<String>>,
    pub prompt: String,
    /// Boat v1 `fast` field.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub fast: Option<Option<bool>>,
    /// Boat v1 `new` field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<bool>,
    /// Boat v1 `conversationId` field.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub conversation_id: Option<Option<String>>,
}
impl fmt::Debug for PromptRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PromptRequest")
            .field("provider", &self.provider)
            .field(
                "model_len",
                &self
                    .model
                    .as_ref()
                    .map(|value| value.as_ref().map(String::len)),
            )
            .field(
                "reasoning_effort_len",
                &self
                    .reasoning_effort
                    .as_ref()
                    .map(|value| value.as_ref().map(String::len)),
            )
            .field("prompt_len", &self.prompt.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub id: String,
    pub prompt_id: String,
    pub prompt_run: PromptRun,
    pub status: String,
    pub provider: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for PromptResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PromptResponse")
            .field("ok", &self.ok)
            .field("type", &self.type_)
            .field("id", &self.id)
            .field("prompt_id", &self.prompt_id)
            .field("status", &self.status)
            .field("provider", &self.provider)
            .field("model_len", &self.model.as_ref().map(String::len))
            .field(
                "reasoning_effort_len",
                &self.reasoning_effort.as_ref().map(String::len),
            )
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptRunResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub id: String,
    pub prompt_run: PromptRun,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for PromptRunResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PromptRunResponse")
            .field("ok", &self.ok)
            .field("type", &self.type_)
            .field("id", &self.id)
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptRun {
    pub id: String,
    pub prompt_id: String,
    pub sandbox_id: String,
    pub status: String,
    pub done: bool,
    pub created_at: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for PromptRun {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PromptRun")
            .field("id", &self.id)
            .field("prompt_id", &self.prompt_id)
            .field("sandbox_id", &self.sandbox_id)
            .field("status", &self.status)
            .field("done", &self.done)
            .field("created_at_len", &self.created_at.as_ref().map(String::len))
            .field("model_len", &self.model.as_ref().map(String::len))
            .field(
                "reasoning_effort_len",
                &self.reasoning_effort.as_ref().map(String::len),
            )
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxEvent {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub type_: String,
    pub timestamp: Option<f64>,
    pub task_id: Option<String>,
    pub data: Option<HashMap<String, serde_json::Value>>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for SandboxEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SandboxEvent")
            .field("id", &self.id)
            .field("type", &self.type_)
            .field("task_id_len", &self.task_id.as_ref().map(String::len))
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventsResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub id: String,
    pub events: Vec<SandboxEvent>,
    pub page_info: Option<PageInfo>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for EventsResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventsResponse")
            .field("ok", &self.ok)
            .field("type", &self.type_)
            .field("id", &self.id)
            .field("events_count", &self.events.len())
            .field("page_info", &self.page_info)
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DesktopRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_access: Option<bool>,
}
impl fmt::Debug for DesktopRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DesktopRequest").finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub success: Option<bool>,
    pub desktop_url: Option<String>,
    pub ip: Option<String>,
    pub mode: Option<String>,
    pub provisioning: Option<bool>,
    pub message: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for DesktopResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DesktopResponse")
            .field("ok", &self.ok)
            .field("type", &self.type_)
            .field("success", &self.success)
            .field(
                "desktop_url_len",
                &self.desktop_url.as_ref().map(String::len),
            )
            .field("ip_len", &self.ip.as_ref().map(String::len))
            .field("mode_len", &self.mode.as_ref().map(String::len))
            .field("provisioning", &self.provisioning)
            .field("message_len", &self.message.as_ref().map(String::len))
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

impl PromptRequest {
    pub fn new(provider: PromptProvider, prompt: impl Into<String>) -> Self {
        Self {
            provider,
            prompt: prompt.into(),
            model: None,
            reasoning_effort: None,
            fast: None,
            new: None,
            conversation_id: None,
        }
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EventsQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}
