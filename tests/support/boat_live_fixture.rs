//! Test-only durable ownership ledger. No credentials, API objects, response bodies,
//! or provider-derived private identity hashes are persisted here.
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use workspace_sdk::{
    boat_client::{Api, CreateOptions, CreateSandboxRequest, DeletionOperation},
    Error, OperationId, Result, WorkspaceId, WorkspaceRef,
};

const MAX_FIXTURES: usize = 2;
const MAX_LEDGER_BYTES: u64 = 64 * 1024;

#[derive(Clone, Copy)]
pub struct Budgets {
    pub request: Duration,
    /// Per-target stop/delete/poll window; one final target-status request may
    /// add `request`. The harness permits at most two registered fixtures.
    pub cleanup: Duration,
    pub poll: Duration,
}
impl Default for Budgets {
    fn default() -> Self {
        Self {
            request: Duration::from_secs(15),
            cleanup: Duration::from_secs(60),
            poll: Duration::from_secs(2),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreationState {
    Intent,
    Uncertain,
    Created,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletionStatus {
    Pending,
    Processing,
    Blocked,
    Completed,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletionStage {
    WaitingForUploads,
    Other,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeletionEvidence {
    pub operation_id: String,
    pub target_id: String,
    pub status: DeletionStatus,
    pub stage: Option<DeletionStage>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupEvidence {
    pub stop_requested: bool,
    pub deletion: Option<DeletionEvidence>,
    /// Fixed SandboxState spelling, or absent when observation failed. A 404 is
    /// never converted to completed deletion.
    pub target_status: Option<String>,
    /// Fixed diagnostic classifications, never raw API/HTTP error text.
    pub errors: BTreeSet<String>,
    pub confirmed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureRecord {
    pub workspace_id: WorkspaceId,
    pub idempotency_key: OperationId,
    pub ttl_seconds: u32,
    pub creation: CreationState,
    pub target_id: Option<String>,
    pub workspace: Option<WorkspaceRef>,
    pub cleanup: CleanupEvidence,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LedgerData {
    version: u32,
    fixtures: Vec<FixtureRecord>,
}

/// Holds a nonblocking exclusive OS file lock for its entire lifetime, including
/// remote effects. Another opener refuses before reading or changing authority.
/// The path must live outside disposable TempDirs. A process crash can leave
/// Uncertain creation; an explicit create_target retry reuses its exact key/body.
pub struct FixtureLedger {
    path: PathBuf,
    data: LedgerData,
    // This stable sibling file must not be removed while any controller is using
    // the ledger. Closing the concrete file releases its OS-backed exclusive lock.
    _lock: File,
}
impl FixtureLedger {
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        let (path, lock) = acquire_ledger(path.as_ref())?;
        let ledger = Self {
            path,
            data: LedgerData {
                version: 1,
                fixtures: vec![],
            },
            _lock: lock,
        };
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options.open(&ledger.path)?;
        file.write_all(&serde_json::to_vec_pretty(&ledger.data)?)?;
        file.sync_all()?;
        File::open(ledger.parent())?.sync_all()?;
        Ok(ledger)
    }
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let (path, lock) = acquire_ledger(path.as_ref())?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = options.open(&path)?;
        if !file.metadata()?.is_file() {
            return failure("fixture ledger must be a regular file");
        }
        let mut bytes = Vec::new();
        file.take(MAX_LEDGER_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_LEDGER_BYTES {
            return failure("fixture ledger exceeds size limit");
        }
        let data: LedgerData =
            serde_json::from_slice(&bytes).map_err(|_| state("invalid fixture ledger encoding"))?;
        if data.fixtures.len() > MAX_FIXTURES {
            return failure("fixture ledger exceeds two-target limit");
        }
        if data.version != 1 {
            return failure("unsupported fixture ledger version");
        }
        let mut workspaces = std::collections::HashSet::new();
        let mut keys = std::collections::HashSet::new();
        let mut targets = std::collections::HashSet::new();
        for fixture in &data.fixtures {
            let has_target = fixture.target_id.is_some();
            let valid_creation = match fixture.creation {
                CreationState::Intent | CreationState::Uncertain => {
                    !has_target && fixture.workspace.is_none() && fixture.cleanup.deletion.is_none()
                }
                CreationState::Created => has_target,
            };
            let valid_deletion = fixture.cleanup.deletion.as_ref().is_none_or(|deletion| {
                fixture.target_id.as_deref() == Some(deletion.target_id.as_str())
                    && OperationId::new(deletion.operation_id.clone()).is_ok()
            });
            let valid_confirmation = !fixture.cleanup.confirmed
                || match fixture.creation {
                    CreationState::Intent => true,
                    CreationState::Uncertain => false,
                    CreationState::Created => fixture
                        .cleanup
                        .deletion
                        .as_ref()
                        .is_some_and(|deletion| deletion.status == DeletionStatus::Completed),
                };
            if fixture.ttl_seconds == 0
                || fixture.ttl_seconds > 300
                || !workspaces.insert(&fixture.workspace_id)
                || !keys.insert(&fixture.idempotency_key)
                || fixture
                    .target_id
                    .as_ref()
                    .is_some_and(|id| !valid_target(id) || !targets.insert(id))
                || fixture
                    .workspace
                    .as_ref()
                    .is_some_and(|workspace| workspace.id != fixture.workspace_id)
                || fixture
                    .cleanup
                    .errors
                    .iter()
                    .any(|error| !known_diagnostic(error))
                || !valid_creation
                || !valid_deletion
                || !valid_confirmation
            {
                return failure("invalid fixture ownership record");
            }
        }
        Ok(Self {
            path,
            data,
            _lock: lock,
        })
    }
    pub fn fixtures(&self) -> &[FixtureRecord] {
        &self.data.fixtures
    }
    pub fn reserve(&mut self, workspace_id: WorkspaceId, ttl_seconds: u32) -> Result<usize> {
        if self.data.fixtures.len() >= MAX_FIXTURES {
            return failure("fixture ledger permits at most two targets");
        }
        if ttl_seconds == 0 || ttl_seconds > 300 {
            return failure("fixture TTL must be between one and 300 seconds");
        }
        if self
            .data
            .fixtures
            .iter()
            .any(|fixture| fixture.workspace_id == workspace_id)
        {
            return failure("fixture workspace identity already reserved");
        }
        let index = self.data.fixtures.len();
        self.data.fixtures.push(FixtureRecord {
            workspace_id,
            idempotency_key: OperationId::generate(),
            ttl_seconds,
            creation: CreationState::Intent,
            target_id: None,
            workspace: None,
            cleanup: CleanupEvidence::default(),
        });
        self.save()?;
        Ok(index)
    }
    pub async fn create_target(
        &mut self,
        api: &Api,
        index: usize,
        budgets: Budgets,
    ) -> Result<String> {
        let fixture = self
            .data
            .fixtures
            .get_mut(index)
            .ok_or_else(|| state("missing fixture intent"))?;
        if fixture.cleanup.confirmed {
            return failure(
                "fixture cleanup already completed; this intent cannot create or attach again",
            );
        }
        if let Some(id) = &fixture.target_id {
            return Ok(id.clone());
        }
        fixture.creation = CreationState::Uncertain;
        let key = fixture.idempotency_key.to_string();
        let request = CreateSandboxRequest {
            ttl_seconds: Some(Some(fixture.ttl_seconds)),
            no_env: Some(true),
            ..Default::default()
        };
        // Intent and exact retry key/body are durable before POST.
        self.save()?;
        let response = tokio::time::timeout(
            budgets.request,
            api.create(
                Some(request),
                &CreateOptions {
                    idempotency_key: Some(key),
                    ..Default::default()
                },
            ),
        )
        .await;
        let response = match response {
            Ok(Ok(response)) if valid_target(&response.sandbox.id) => response,
            _ => {
                return failure("fixture creation uncertain; reconcile using the recorded intent and same idempotency key");
            }
        };
        let accepted = response.ok;
        let id = response.sandbox.id;
        let fixture = &mut self.data.fixtures[index];
        fixture.target_id = Some(id.clone());
        fixture.creation = CreationState::Created;
        // Register raw target ownership before any SDK bootstrap can fail.
        self.save()?;
        if !accepted {
            return failure("fixture create returned a target with unconfirmed acceptance; target retained for cleanup");
        }
        Ok(id)
    }
    pub fn record_workspace(&mut self, index: usize, workspace: WorkspaceRef) -> Result<()> {
        let fixture = self
            .data
            .fixtures
            .get_mut(index)
            .ok_or_else(|| state("missing fixture intent"))?;
        if fixture.workspace_id != workspace.id
            || fixture.target_id.is_none()
            || fixture.cleanup.confirmed
        {
            return failure("workspace does not match registered fixture");
        }
        fixture.workspace = Some(workspace);
        self.save()
    }
    /// Attempt every registered raw target even after an earlier target fails.
    /// At most two targets: each uses `cleanup` plus one final `request` budget.
    /// Pending/blocked/unknown deletion, missing target reads and stop acceptance
    /// never mean cleanup completed. Unknown creation is retained for reconciliation.
    pub async fn cleanup_all(&mut self, api: &Api, budgets: Budgets) -> Result<()> {
        let mut persistence_failed = false;
        for index in (0..self.data.fixtures.len()).rev() {
            let fixture = &self.data.fixtures[index];
            if fixture.cleanup.confirmed {
                continue;
            }
            let Some(target) = fixture.target_id.clone() else {
                if fixture.creation == CreationState::Uncertain {
                    self.data.fixtures[index]
                        .cleanup
                        .errors
                        .insert("creation_unresolved".into());
                } else {
                    self.data.fixtures[index].cleanup.confirmed = true;
                }
                persistence_failed |= self.save().is_err();
                continue;
            };
            let deadline = Instant::now() + budgets.cleanup;
            match tokio::time::timeout(
                remaining(deadline, budgets.request),
                api.stop(&target, None),
            )
            .await
            {
                Ok(Ok(response)) if response.ok && response.id == target => {
                    self.data.fixtures[index].cleanup.stop_requested = true;
                }
                _ => {
                    self.data.fixtures[index]
                        .cleanup
                        .errors
                        .insert("stop_unconfirmed".into());
                }
            }
            persistence_failed |= self.save().is_err();
            let previous = self.data.fixtures[index].cleanup.deletion.clone();
            let operation = if let Some(previous) = previous {
                tokio::time::timeout(
                    remaining(deadline, budgets.request),
                    api.get_deletion_operation(&previous.operation_id),
                )
                .await
            } else {
                tokio::time::timeout(
                    remaining(deadline, budgets.request),
                    api.delete_sandbox(&target),
                )
                .await
            };
            let mut response = match operation {
                Ok(Ok(value)) => Some(value),
                _ => {
                    self.data.fixtures[index]
                        .cleanup
                        .errors
                        .insert("deletion_request_unconfirmed".into());
                    None
                }
            };
            while let Some(value) = response.take() {
                let expected = self.data.fixtures[index]
                    .cleanup
                    .deletion
                    .as_ref()
                    .map(|entry| entry.operation_id.as_str());
                let Some(observation) =
                    deletion_evidence(value.ok, value.operation, &target, expected)
                else {
                    self.data.fixtures[index]
                        .cleanup
                        .errors
                        .insert("deletion_identity_mismatch".into());
                    break;
                };
                let complete = observation.status == DeletionStatus::Completed;
                let operation_id = observation.operation_id.clone();
                self.data.fixtures[index].cleanup.deletion = Some(observation);
                self.data.fixtures[index].cleanup.confirmed = complete;
                persistence_failed |= self.save().is_err();
                if complete || Instant::now() >= deadline {
                    break;
                }
                tokio::time::sleep(
                    budgets
                        .poll
                        .min(deadline.saturating_duration_since(Instant::now())),
                )
                .await;
                if Instant::now() >= deadline {
                    break;
                }
                response = match tokio::time::timeout(
                    remaining(deadline, budgets.request),
                    api.get_deletion_operation(&operation_id),
                )
                .await
                {
                    Ok(Ok(value)) => Some(value),
                    _ => {
                        self.data.fixtures[index]
                            .cleanup
                            .errors
                            .insert("deletion_poll_unconfirmed".into());
                        None
                    }
                };
            }
            match tokio::time::timeout(budgets.request, api.get(&target)).await {
                Ok(Ok(value)) if value.ok && value.sandbox.id == target => {
                    self.data.fixtures[index].cleanup.target_status =
                        Some(value.sandbox.state.as_str().into());
                    if value.sandbox.state.is_operable()
                        && self.data.fixtures[index].cleanup.confirmed
                    {
                        self.data.fixtures[index].cleanup.confirmed = false;
                        self.data.fixtures[index]
                            .cleanup
                            .errors
                            .insert("completed_deletion_target_still_operable".into());
                    }
                }
                _ => {
                    self.data.fixtures[index]
                        .cleanup
                        .errors
                        .insert("target_status_unavailable".into());
                }
            }
            persistence_failed |= self.save().is_err();
        }
        if persistence_failed {
            return failure("fixture cleanup evidence could not be durably saved");
        }
        if !self
            .data
            .fixtures
            .iter()
            .all(|fixture| fixture.cleanup.confirmed)
        {
            return failure(
                "fixture cleanup incomplete; inspect durable target and deletion status",
            );
        }
        Ok(())
    }
    fn parent(&self) -> &Path {
        self.path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
    }
    fn save(&self) -> Result<()> {
        let mut pending = tempfile::NamedTempFile::new_in(self.parent())?;
        let bytes = serde_json::to_vec_pretty(&self.data)?;
        if bytes.len() as u64 > MAX_LEDGER_BYTES {
            return failure("fixture ledger exceeds size limit");
        }
        pending.write_all(&bytes)?;
        pending.as_file().sync_all()?;
        pending
            .persist(&self.path)
            .map_err(|error| Error::Io(error.error))?;
        File::open(self.parent())?.sync_all()?;
        Ok(())
    }
}
fn acquire_ledger(path: &Path) -> Result<(PathBuf, File)> {
    let name = path
        .file_name()
        .ok_or_else(|| state("fixture ledger requires a file name"))?;
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent)?;
    let path = parent.join(name);
    let mut lock_name = name.to_os_string();
    lock_name.push(".lock");
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let lock = options.open(parent.join(lock_name))?;
    if !lock.metadata()?.is_file() {
        return failure("fixture ledger lock must be a regular file");
    }
    lock.try_lock_exclusive().map_err(|error| {
        if error.kind() == std::io::ErrorKind::WouldBlock {
            Error::Conflict("fixture ledger is controlled by another operation".into())
        } else {
            Error::Io(error)
        }
    })?;
    Ok((path, lock))
}
fn known_diagnostic(value: &str) -> bool {
    matches!(
        value,
        "creation_unresolved"
            | "stop_unconfirmed"
            | "deletion_request_unconfirmed"
            | "deletion_identity_mismatch"
            | "deletion_poll_unconfirmed"
            | "completed_deletion_target_still_operable"
            | "target_status_unavailable"
    )
}
fn remaining(deadline: Instant, request: Duration) -> Duration {
    request.min(deadline.saturating_duration_since(Instant::now()))
}
fn valid_target(id: &str) -> bool {
    id.len() == 11
        && id.starts_with("bx_")
        && id[3..]
            .bytes()
            .all(|byte| b"23456789abcdefghjkmnpqrstuvwxyz".contains(&byte))
}
fn deletion_evidence(
    ok: bool,
    operation: DeletionOperation,
    target: &str,
    expected: Option<&str>,
) -> Option<DeletionEvidence> {
    if !ok
        || operation.kind != "sandbox"
        || operation.target_id != target
        || OperationId::new(operation.id.clone()).is_err()
        || expected.is_some_and(|id| id != operation.id)
    {
        return None;
    }
    let status = match operation.status.as_str() {
        "pending" => DeletionStatus::Pending,
        "processing" => DeletionStatus::Processing,
        "blocked" => DeletionStatus::Blocked,
        "completed" => DeletionStatus::Completed,
        _ => DeletionStatus::Unknown,
    };
    let stage = operation
        .extra
        .get("stage")
        .or_else(|| {
            operation
                .extra
                .get("progress")
                .and_then(|value| value.get("stage"))
        })
        .and_then(serde_json::Value::as_str)
        .map(|stage| {
            if stage == "waiting_for_uploads" {
                DeletionStage::WaitingForUploads
            } else {
                DeletionStage::Other
            }
        });
    Some(DeletionEvidence {
        operation_id: operation.id,
        target_id: operation.target_id,
        status,
        stage,
    })
}
fn state(message: &str) -> Error {
    Error::State(message.into())
}
fn failure<T>(message: &str) -> Result<T> {
    Err(state(message))
}
