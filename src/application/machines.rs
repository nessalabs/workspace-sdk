//! Counted use of SDK-owned machines and the one path that rests them.
use super::{OwnedWorkspace, WorkspaceSdk};
use crate::{domain::WorkspaceId, Error, Result};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex as SyncMutex},
    time::Duration,
};
use tokio::{sync::Mutex, time::Instant};

/// Keeps SDK-owned machines awake while something uses them, and stops each one
/// after it has had no use for the idle time the caller chose.
///
/// Every use is a named [`MachineUse`] guard. A machine is in use exactly while
/// at least one guard for it is alive; nothing reads reference counts. Opening a
/// machine for use and resting it take the same per-machine lock, so a rest can
/// never stop a machine that a new use has just opened. The provider's own
/// auto-stop lifetime remains only a backstop for a process that dies.
pub struct Machines {
    sdk: Arc<WorkspaceSdk>,
    idle: Duration,
    entries: SyncMutex<HashMap<WorkspaceId, Arc<Entry>>>,
}

/// One named use of a machine. Dropping it ends the use.
#[must_use = "the machine may rest as soon as its use is dropped"]
pub struct MachineUse {
    entry: Arc<Entry>,
    id: u64,
    workspace: OwnedWorkspace,
}
impl MachineUse {
    /// The machine this use keeps awake.
    pub fn workspace(&self) -> &OwnedWorkspace {
        &self.workspace
    }
}
impl std::ops::Deref for MachineUse {
    type Target = OwnedWorkspace;
    fn deref(&self) -> &OwnedWorkspace {
        &self.workspace
    }
}
impl Drop for MachineUse {
    fn drop(&mut self) {
        // Ending a use only ever moves a machine toward rest, so it needs no
        // ordering with opening or resting beyond this short lock.
        if let Ok(mut uses) = self.entry.uses.lock() {
            uses.holders.remove(&self.id);
            if uses.holders.is_empty() {
                uses.idle_since = Some(Instant::now());
            }
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

struct Entry {
    /// Orders opening for use against resting.
    lifecycle: Mutex<Lifecycle>,
    uses: SyncMutex<Uses>,
}
#[derive(Default)]
struct Lifecycle {
    /// The open handle, kept while the machine is awake.
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
    /// Start a named use of a saved SDK-owned machine, waking it if it rests.
    pub async fn hold(&self, id: &WorkspaceId, name: impl Into<String>) -> Result<MachineUse> {
        let entry = self.entry(id)?;
        let mut lifecycle = entry.lifecycle.lock().await;
        let workspace = match &lifecycle.awake {
            Some(workspace) => workspace.clone(),
            None => {
                let workspace = self.sdk.owned_workspace(id).await?;
                workspace.ensure_ready().await?;
                lifecycle.awake = Some(workspace.clone());
                workspace
            }
        };
        let mut uses = entry.uses.lock().map_err(poisoned)?;
        let use_id = uses.next;
        uses.next += 1;
        uses.holders.insert(use_id, name.into());
        uses.idle_since = None;
        drop(uses);
        Ok(MachineUse {
            entry: entry.clone(),
            id: use_id,
            workspace,
        })
    }
    /// The names of the current uses of a machine.
    pub fn uses(&self, id: &WorkspaceId) -> Result<Vec<String>> {
        let entries = self.entries.lock().map_err(poisoned)?;
        let Some(entry) = entries.get(id) else {
            return Ok(Vec::new());
        };
        let uses = entry.uses.lock().map_err(poisoned)?;
        Ok(uses.holders.values().cloned().collect())
    }
    /// The one rest path: stop every awake machine that has had no use for the
    /// idle time. Call it on a timer, or use [`Self::rest_every`]. A failed stop
    /// leaves the machine awake, so the next call tries again.
    pub async fn rest_idle(&self) -> Result<RestReport> {
        let entries: Vec<_> = self
            .entries
            .lock()
            .map_err(poisoned)?
            .iter()
            .map(|(id, entry)| (id.clone(), entry.clone()))
            .collect();
        let mut report = RestReport::default();
        for (id, entry) in entries {
            let mut lifecycle = entry.lifecycle.lock().await;
            let Some(workspace) = &lifecycle.awake else {
                continue;
            };
            let due = {
                let uses = entry.uses.lock().map_err(poisoned)?;
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
        Ok(report)
    }
    /// Run [`Self::rest_idle`] every `period` until the returned task is
    /// aborted, handing each report that rested or failed anything to `report`.
    pub fn rest_every(
        self: &Arc<Self>,
        period: Duration,
        mut report: impl FnMut(Result<RestReport>) + Send + 'static,
    ) -> tokio::task::JoinHandle<()> {
        let machines = self.clone();
        tokio::spawn(async move {
            let mut ticks = tokio::time::interval(period);
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticks.tick().await;
                let outcome = machines.rest_idle().await;
                if outcome.as_ref().map_or(true, |done| {
                    !done.rested.is_empty() || !done.failed.is_empty()
                }) {
                    report(outcome);
                }
            }
        })
    }
    fn entry(&self, id: &WorkspaceId) -> Result<Arc<Entry>> {
        Ok(self
            .entries
            .lock()
            .map_err(poisoned)?
            .entry(id.clone())
            .or_insert_with(|| {
                Arc::new(Entry {
                    lifecycle: Mutex::new(Lifecycle::default()),
                    uses: SyncMutex::new(Uses::default()),
                })
            })
            .clone())
    }
}

fn poisoned<T>(_: T) -> Error {
    Error::State("machine use lock poisoned".into())
}
