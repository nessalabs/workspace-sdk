use super::*;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotSummary {
    pub id: String,
    pub sandbox_id: String,
    pub status: String,
    pub kind: Option<String>,
    pub generation: Option<u64>,
    pub chain_id: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub size_bytes: Option<u64>,
    pub file_count: Option<u64>,
    pub content_size_bytes: Option<u64>,
    pub content_file_count: Option<u64>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for SnapshotSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SnapshotSummary")
            .field("id", &self.id)
            .field("sandbox_id", &self.sandbox_id)
            .field("status", &self.status)
            .field("kind", &self.kind)
            .field("generation", &self.generation)
            .field("chain_id_len", &self.chain_id.as_ref().map(String::len))
            .field("created_at_len", &self.created_at.len())
            .field(
                "completed_at_len",
                &self.completed_at.as_ref().map(String::len),
            )
            .field("size_bytes", &self.size_bytes)
            .field("file_count", &self.file_count)
            .field("content_size_bytes", &self.content_size_bytes)
            .field("content_file_count", &self.content_file_count)
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotListResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub snapshots: Vec<SnapshotSummary>,
    pub page_info: Option<PageInfo>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for SnapshotListResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SnapshotListResponse")
            .field("ok", &self.ok)
            .field("type", &self.type_)
            .field("snapshots_count", &self.snapshots.len())
            .field("page_info", &self.page_info)
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotLatestResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub snapshot: Option<SnapshotSummary>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for SnapshotLatestResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SnapshotLatestResponse")
            .field("ok", &self.ok)
            .field("type", &self.type_)
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotTreeEntry {
    pub path: String,
    pub kind: String,
    pub size: Option<u64>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for SnapshotTreeEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SnapshotTreeEntry")
            .field("path_len", &self.path.len())
            .field("kind", &self.kind)
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotTreeResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub snapshot_id: String,
    pub sandbox_id: String,
    pub generation: u64,
    pub tree_available: bool,
    pub truncated: bool,
    pub file_count: u64,
    pub total_size_bytes: u64,
    pub entries: Vec<SnapshotTreeEntry>,
    pub reason: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for SnapshotTreeResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SnapshotTreeResponse")
            .field("ok", &self.ok)
            .field("type", &self.type_)
            .field("snapshot_id", &self.snapshot_id)
            .field("sandbox_id", &self.sandbox_id)
            .field("generation", &self.generation)
            .field("tree_available", &self.tree_available)
            .field("truncated", &self.truncated)
            .field("file_count", &self.file_count)
            .field("total_size_bytes", &self.total_size_bytes)
            .field("entries_count", &self.entries.len())
            .field("reason", &self.reason)
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotChunk {
    pub snapshot_id: String,
    pub generation: u64,
    pub chunk_index: u64,
    pub r2_key: String,
    pub size_bytes: Option<u64>,
    pub sha256: Option<String>,
    pub signed_url: String,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for SnapshotChunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SnapshotChunk")
            .field("snapshot_id", &self.snapshot_id)
            .field("generation", &self.generation)
            .field("chunk_index", &self.chunk_index)
            .field("r2_key_len", &self.r2_key.len())
            .field("size_bytes", &self.size_bytes)
            .field("sha256_len", &self.sha256.as_ref().map(String::len))
            .field("signed_url_len", &self.signed_url.len())
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotInventory {
    pub r2_key: String,
    pub signed_url: String,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for SnapshotInventory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SnapshotInventory")
            .field("r2_key_len", &self.r2_key.len())
            .field("signed_url_len", &self.signed_url.len())
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotDownloadResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub snapshot_id: String,
    pub sandbox_id: String,
    pub kind: String,
    pub generation: u64,
    pub expires_in_seconds: u64,
    pub reconstruct: String,
    pub inventory: Option<SnapshotInventory>,
    pub chunks: Vec<SnapshotChunk>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for SnapshotDownloadResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SnapshotDownloadResponse")
            .field("ok", &self.ok)
            .field("type", &self.type_)
            .field("snapshot_id", &self.snapshot_id)
            .field("sandbox_id", &self.sandbox_id)
            .field("kind", &self.kind)
            .field("generation", &self.generation)
            .field("expires_in_seconds", &self.expires_in_seconds)
            .field("reconstruct_len", &self.reconstruct.len())
            .field("chunks_count", &self.chunks.len())
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionOperation {
    pub id: String,
    pub kind: String,
    pub target_id: String,
    pub reason: String,
    pub status: String,
    pub attempt_count: u64,
    pub requested_at: String,
    pub completed_at: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for DeletionOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeletionOperation")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("target_id", &self.target_id)
            .field("reason", &self.reason)
            .field("status", &self.status)
            .field("attempt_count", &self.attempt_count)
            .field("requested_at_len", &self.requested_at.len())
            .field(
                "completed_at_len",
                &self.completed_at.as_ref().map(String::len),
            )
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionOperationResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub operation: DeletionOperation,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for DeletionOperationResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeletionOperationResponse")
            .field("ok", &self.ok)
            .field("type", &self.type_)
            .field("extra_count", &self.extra.len())
            .finish()
    }
}

/// Shared options for global and per-sandbox snapshot history.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnapshotsQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
}
/// The real response for each branch of `stop_and_remove_with`.
#[derive(Debug, Clone)]
pub enum StopOrDeleteResponse {
    Stopped(std::boxed::Box<SandboxActionResponse>),
    Deleting(std::boxed::Box<DeletionOperationResponse>),
}
