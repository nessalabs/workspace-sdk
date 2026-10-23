use super::Workspace;
use crate::{DirectoryEntry, Error, FileRevision, ListDirectory, Result, WorkspacePath};
use std::collections::BTreeMap;

/// Caller-driven directory reconciliation. Call `poll` on the application's timer.
/// No task, OS watcher, socket, queue, or file survives dropping this value.
/// At most 4096 immediate children are retained. This observes current state;
/// short-lived changes between polls and rename identity are not an event log.
/// Construct a new observer after restart for a reset snapshot. On transport or
/// stale-pagination errors the prior baseline is retained; retry a complete poll.
pub struct DirectoryObserver {
    workspace: Workspace,
    path: WorkspacePath,
    baseline: Option<BTreeMap<String, DirectoryEntry>>,
}
/// A complete directory reconciliation. Rename appears as removal plus upsert.
/// `reset` means the consumer must discard its old view and use `upserted`.
#[derive(Clone, Debug)]
pub struct DirectoryChanges {
    /// True on the first successful poll after construction.
    pub reset: bool,
    /// Newly observed or modified entries.
    pub upserted: Vec<DirectoryEntry>,
    /// Previously observed paths now absent.
    pub removed: Vec<WorkspacePath>,
    /// Revision of the complete directory observation.
    pub revision: FileRevision,
}
impl DirectoryObserver {
    pub(crate) fn new(workspace: Workspace, path: WorkspacePath) -> Self {
        Self {
            workspace,
            path,
            baseline: None,
        }
    }
    /// Fetch a complete bounded snapshot before advancing the baseline. No background
    /// work is spawned; canceling this future leaves the baseline unchanged.
    pub async fn poll(&mut self) -> Result<DirectoryChanges> {
        let mut current = BTreeMap::new();
        let mut cursor = None;
        let mut revision = None;
        let mut pages = 0;
        loop {
            pages += 1;
            if pages > 16 {
                return Err(Error::State(
                    "directory observation exceeds 16 pages".into(),
                ));
            }
            let request = ListDirectory {
                path: self.path.clone(),
                limit: 256,
                cursor: cursor.clone(),
            };
            let page = self.workspace.list_directory(request.clone()).await?;
            page.validate_for(&request)?;
            if revision
                .as_ref()
                .is_some_and(|value| *value != page.revision)
            {
                return Err(Error::Conflict(
                    "directory changed during observation".into(),
                ));
            }
            if page.entries.len() > 256 {
                return Err(Error::State("oversized directory page".into()));
            }
            let prior_offset = current.len();
            revision = Some(page.revision);

            for entry in page.entries {
                let parent = std::path::Path::new(entry.path.as_str())
                    .parent()
                    .and_then(|value| value.to_str())
                    .unwrap_or("");
                let expected_parent = if self.path == WorkspacePath::root() {
                    ""
                } else {
                    self.path.as_str()
                };
                if parent != expected_parent || entry.path == WorkspacePath::root() {
                    return Err(Error::State(
                        "directory page contains a non-child path".into(),
                    ));
                }
                if current.len() >= 4096
                    || current
                        .insert(entry.path.as_str().to_owned(), entry)
                        .is_some()
                {
                    return Err(Error::State(
                        "invalid or oversized directory observation".into(),
                    ));
                }
            }
            match page.next {
                Some(next)
                    if next.offset as usize == current.len()
                        && current.len() > prior_offset
                        && Some(&next.revision) == revision.as_ref() =>
                {
                    cursor = Some(next)
                }
                Some(_) => return Err(Error::State("non-advancing directory cursor".into())),
                None => break,
            }
        }
        let upserted = current
            .iter()
            .filter(|(key, entry)| {
                self.baseline.as_ref().and_then(|old| old.get(*key)) != Some(*entry)
            })
            .map(|(_, entry)| entry.clone())
            .collect();
        let removed = self
            .baseline
            .as_ref()
            .map(|old| {
                old.iter()
                    .filter(|(key, _)| !current.contains_key(*key))
                    .map(|(_, entry)| entry.path.clone())
                    .collect()
            })
            .unwrap_or_default();
        let reset = self.baseline.is_none();
        self.baseline = Some(current);
        Ok(DirectoryChanges {
            reset,
            upserted,
            removed,
            revision: revision.expect("at least one page"),
        })
    }
}
