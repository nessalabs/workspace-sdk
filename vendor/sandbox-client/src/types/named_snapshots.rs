//! Request and response types for named snapshots.
use super::*;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedSnapshotDeletedResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: String,
    pub name: String,
    pub status: String,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for NamedSnapshotDeletedResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NamedSnapshotDeletedResponse")
            .field("ok", &self.ok)
            .field("type__len", &self.type_.len())
            .field("name_len", &self.name.len())
            .field("status", &self.status)
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedSnapshotInfoResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: String,
    pub snapshot: NamedSnapshot,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for NamedSnapshotInfoResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NamedSnapshotInfoResponse")
            .field("ok", &self.ok)
            .field("type__len", &self.type_.len())
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedSnapshot {
    pub name: String,
    pub status: String,
    pub error: Option<String>,
    pub source_sandbox_id: String,
    pub snapshot_id: Option<String>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub size_bytes: Option<u64>,
    pub created_at: String,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for NamedSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NamedSnapshot")
            .field("name_len", &self.name.len())
            .field("status", &self.status)
            .field("source_box_id_len", &self.source_sandbox_id.len())
            .field("size_bytes", &self.size_bytes)
            .field("created_at_len", &self.created_at.len())
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedSnapshotListResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: String,
    pub snapshots: Vec<NamedSnapshot>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for NamedSnapshotListResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NamedSnapshotListResponse")
            .field("ok", &self.ok)
            .field("type__len", &self.type_.len())
            .field("snapshots_count", &self.snapshots.len())
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedSnapshotSaveRequest {
    pub sandbox_id: String,
    pub name: String,
}
impl fmt::Debug for NamedSnapshotSaveRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NamedSnapshotSaveRequest")
            .field("box_id_len", &self.sandbox_id.len())
            .field("name_len", &self.name.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedSnapshotSavingResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: String,
    pub status: String,
    pub snapshot: NamedSnapshot,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for NamedSnapshotSavingResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NamedSnapshotSavingResponse")
            .field("ok", &self.ok)
            .field("type__len", &self.type_.len())
            .field("status", &self.status)
            .field("extra_count", &self.extra.len())
            .finish_non_exhaustive()
    }
}
