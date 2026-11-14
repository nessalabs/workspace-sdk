//! Named uses of SDK-owned machines and the one path that rests them.
use super::{
    uses::{NamedUse, Uses},
    OwnedWorkspace, Workspace, WorkspaceSdk,
};
use crate::{domain::WorkspaceId, Error, Result};
use std::{sync::Arc, time::Duration};

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
    /// Each machine's uses, and behind its turn its open handle, kept from
    /// the moment waking starts until a rest stops it, so a wake that fails
    /// part way is still rested.
    uses: Uses<WorkspaceId, Option<OwnedWorkspace>>,
}

/// One named use of a machine. Dropping it ends the use.
///
/// It gives the workspace's operations through `Deref`, but not the owned
/// handle's stop or destroy: resting is [`Machines::rest_idle`]'s alone.
/// The handle gives no path to the backend's stop or delete, and the compiler
/// checks that (code that also holds the provider can still connect and stop
/// the target itself). A connection opened through the handle can outlive the
/// use; once the use is dropped the machine may be rested under it. A holder
/// can use the workspace:
///
/// ```no_run
/// async fn work(holder: workspace_sdk::MachineUse) {
///     let _ = holder.state().await;
///     let _ = holder.runtime_host();
/// }
/// ```
///
/// but none of these compile:
///
/// ```compile_fail,E0599
/// async fn rest(holder: workspace_sdk::MachineUse) {
///     holder.stop().await.unwrap();
/// }
/// ```
///
/// ```compile_fail,E0599
/// fn reach(holder: &workspace_sdk::MachineUse) {
///     let _ = holder.backend();
/// }
/// ```
///
/// ```compile_fail,E0599
/// fn reach(holder: &workspace_sdk::MachineUse) {
///     let _ = holder.runtime_host().backend();
/// }
/// ```
#[must_use = "the machine may rest as soon as its use is dropped"]
pub struct MachineUse {
    _use: NamedUse,
    workspace: Workspace,
}
impl std::ops::Deref for MachineUse {
    type Target = Workspace;
    fn deref(&self) -> &Workspace {
        &self.workspace
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

impl Machines {
    /// Rest machines after `idle` without use.
    pub fn new(sdk: Arc<WorkspaceSdk>, idle: Duration) -> Self {
        Self {
            sdk,
            idle,
            uses: Uses::new(),
        }
    }
    /// Start a named use of a saved machine this SDK created, waking it if
    /// nothing was using it.
    pub async fn hold(&self, id: &WorkspaceId, name: impl Into<String>) -> Result<MachineUse> {
        let name = name.into();
        // Refuse an attached or unknown ID before keeping anything for it.
        self.sdk.owned_record(id)?;
        let mut turn = self.uses.turn(id).await;
        let awake = (*turn).clone();
        let workspace = match awake {
            Some(workspace) if turn.in_use() => workspace,
            awake => {
                // Nothing holds it, so something else may have stopped it:
                // make sure it is ready before handing it out.
                let workspace = match awake {
                    Some(workspace) => workspace,
                    None => self.sdk.owned_workspace(id).await?,
                };
                // From here it may be awake. If this wake fails, panics or is
                // cancelled, the rest path must still stop it, so the idle
                // clock runs until a use is recorded, and a clock already
                // running is kept: a failed wake of a machine that was idle
                // long enough is stopped on the next pass.
                *turn = Some(workspace.clone());
                turn.waking();
                workspace.ensure_ready().await?;
                workspace
            }
        };
        Ok(MachineUse {
            _use: turn.hold(name),
            workspace: workspace.into(),
        })
    }
    /// The names of the current uses of a machine.
    pub fn uses(&self, id: &WorkspaceId) -> Vec<String> {
        self.uses.names(id)
    }
    /// The one rest path: stop every awake machine that has had no use for the
    /// idle time. Call it on a timer, or use [`Self::rest_every`]. A machine
    /// being opened right now is skipped, since it is about to be used. A
    /// failed stop leaves the machine awake, so the next call tries again.
    pub async fn rest_idle(&self) -> RestReport {
        let mut report = RestReport::default();
        for id in self.uses.idle(self.idle) {
            let Some(mut turn) = self.uses.try_turn(&id) else {
                continue;
            };
            let Some(workspace) = (*turn).clone() else {
                continue;
            };
            // A use may have been taken since the list was made.
            if !turn.due(self.idle) {
                continue;
            }
            match workspace.stop().await {
                Ok(()) => {
                    *turn = None;
                    turn.rested();
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
}
