//! Counted use of SDK-owned machines and the one path that rests them.
use super::{OwnedWorkspace, Workspace, WorkspaceSdk};
use crate::{domain::WorkspaceId, Error, Result};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex as SyncMutex, MutexGuard, PoisonError},
    time::Duration,
};
use tokio::{sync::Mutex, time::Instant};

/// Keeps SDK-owned machines awake while something uses them, and stops each one
/// after it has had no use for the idle time the caller chose.
///
/// Every use is a named [`MachineUse`] guard. A machine is in use exactly while
/// at least one guard for it is alive; nothing reads reference counts. Opening a
/// machine for use and resting it take the same per-machine lock, so a rest can
/// never stop a machine that a new use has just opened. That lock lives in this
/// value, so keep one `Machines` per state store; a second one, in this process
/// or another, does not see these uses. The provider's own auto-stop lifetime
/// remains only a backstop for a process that dies.
pub struct Machines {
    sdk: Arc<WorkspaceSdk>,
    idle: Duration,
    entries: SyncMutex<HashMap<WorkspaceId, Arc<Entry>>>,
}

/// One named use of a machine. Dropping it ends the use.
///
/// It gives the workspace's operations through `Deref`, but not stop or
/// destroy: resting is [`Machines::rest_idle`]'s alone.
#[must_use = "the machine may rest as soon as its use is dropped"]
pub struct MachineUse {
    entry: Arc<Entry>,
    id: u64,
    workspace: Workspace,
}
impl std::ops::Deref for MachineUse {
    type Target = Workspace;
    fn deref(&self) -> &Workspace {
        &self.workspace
    }
}
impl Drop for MachineUse {
    fn drop(&mut self) {
        // Ending a use only ever moves a machine toward rest, so it needs no
        // ordering with opening or resting beyond this short lock. A poisoned
        // lock still holds consistent uses: every change under it is one step.
        let mut uses = lock(&self.entry.uses);
        uses.holders.remove(&self.id);
        if uses.holders.is_empty() {
            uses.idle_since = Some(Instant::now());
        }
    }
}

/// What one pass of [`Machines::rest_idle`] did.
#[derive(Debug, Default)]
pub struct RestReport {
    /// Machines that were stopped.
    pub rested: Vec<WorkspaceId>,
    /// Machines whose stop failed; they stay awake and are tried again.
    pub failed: Vec<(WorkspaceId, Error)>,
}

/// The background rest started by [`Machines::rest_every`]. Dropping it stops
/// the timer.
#[must_use = "dropping the task stops resting"]
pub struct RestTask(tokio::task::JoinHandle<()>);
impl Drop for RestTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct Entry {
    /// Orders opening for use against resting.
    lifecycle: Mutex<Lifecycle>,
    uses: SyncMutex<Uses>,
}
#[derive(Default)]
struct Lifecycle {
    /// The open handle, kept from the moment waking starts until a rest stops
    /// the machine, so a wake that fails part way is still rested.
    awake: Option<OwnedWorkspace>,
}
#[derive(Default)]
struct Uses {
    next: u64,
    holders: BTreeMap<u64, String>,
    /// When the last use ended. `None` while in use or before any use.
    idle_since: Option<Instant>,
}

impl Machines {
    /// Rest machines after `idle` without use.
    pub fn new(sdk: Arc<WorkspaceSdk>, idle: Duration) -> Self {
        Self {
            sdk,
            idle,
            entries: SyncMutex::new(HashMap::new()),
        }
    }
    /// Start a named use of a saved machine this SDK created, waking it if
    /// nothing was using it.
    pub async fn hold(&self, id: &WorkspaceId, name: impl Into<String>) -> Result<MachineUse> {
        let name = name.into();
        // Refuse an attached or unknown ID before keeping anything for it.
        self.sdk.owned_record(id)?;
        let entry = self.entry(id);
        let mut lifecycle = entry.lifecycle.lock().await;
        let in_use = !lock(&entry.uses).holders.is_empty();
        let workspace = match &lifecycle.awake {
            Some(workspace) if in_use => workspace.clone(),
            awake => {
                // Nothing holds it, so something else may have stopped it:
                // make sure it is ready before handing it out.
                let workspace = match awake {
                    Some(workspace) => workspace.clone(),
                    None => self.sdk.owned_workspace(id).await?,
                };
                lifecycle.awake = Some(workspace.clone());
                if let Err(error) = workspace.ensure_ready().await {
                    // It may be partly awake; let the rest path stop it.
                    let mut uses = lock(&entry.uses);
                    if uses.holders.is_empty() {
                        uses.idle_since = Some(Instant::now());
                    }
                    return Err(error);
                }
                workspace
            }
        };
        let mut uses = lock(&entry.uses);
        let use_id = uses.next;
        uses.next += 1;
        uses.holders.insert(use_id, name);
        uses.idle_since = None;
        drop(uses);
        Ok(MachineUse {
            entry: entry.clone(),
            id: use_id,
            workspace: workspace.into(),
        })
    }
    /// The names of the current uses of a machine.
    pub fn uses(&self, id: &WorkspaceId) -> Vec<String> {
        let Some(entry) = lock(&self.entries).get(id).cloned() else {
            return Vec::new();
        };
        let names = lock(&entry.uses).holders.values().cloned().collect();
        names
    }
    /// The one rest path: stop every awake machine that has had no use for the
    /// idle time. Call it on a timer, or use [`Self::rest_every`]. A machine
    /// being opened right now is skipped, since it is about to be used. A
    /// failed stop leaves the machine awake, so the next call tries again.
    pub async fn rest_idle(&self) -> RestReport {
        let entries: Vec<_> = lock(&self.entries)
            .iter()
            .map(|(id, entry)| (id.clone(), entry.clone()))
            .collect();
        let mut report = RestReport::default();
        for (id, entry) in entries {
            let Ok(mut lifecycle) = entry.lifecycle.try_lock() else {
                continue;
            };
            let Some(workspace) = &lifecycle.awake else {
                continue;
            };
            let due = {
                let uses = lock(&entry.uses);
                uses.holders.is_empty()
                    && uses
                        .idle_since
                        .is_some_and(|since| since.elapsed() >= self.idle)
            };
            if !due {
                continue;
            }
            match workspace.stop().await {
                Ok(()) => {
                    lifecycle.awake = None;
                    report.rested.push(id);
                }
                Err(error) => report.failed.push((id, error)),
            }
        }
        report
    }
    /// Run [`Self::rest_idle`] every `period` until the returned task is
    /// dropped, handing each report that rested or failed anything to `report`.
    pub fn rest_every(
        self: &Arc<Self>,
        period: Duration,
        mut report: impl FnMut(RestReport) + Send + 'static,
    ) -> RestTask {
        let machines = self.clone();
        RestTask(tokio::spawn(async move {
            let mut ticks = tokio::time::interval(period);
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticks.tick().await;
                let done = machines.rest_idle().await;
                if !done.rested.is_empty() || !done.failed.is_empty() {
                    report(done);
                }
            }
        }))
    }
    fn entry(&self, id: &WorkspaceId) -> Arc<Entry> {
        lock(&self.entries)
            .entry(id.clone())
            .or_insert_with(|| {
                Arc::new(Entry {
                    lifecycle: Mutex::new(Lifecycle::default()),
                    uses: SyncMutex::new(Uses::default()),
                })
            })
            .clone()
    }
}

/// Take a short lock. Every change under these locks is a single step, so a
/// panic elsewhere cannot leave them half-changed.
fn lock<T>(mutex: &SyncMutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
