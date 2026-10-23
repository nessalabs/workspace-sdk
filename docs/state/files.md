---
id: workspace-files
title: Browse and observe workspace files
kind: feature
status: implemented
summary: Read bounded directory pages and binary chunks, reconcile external changes, and create files without replacing another writer's content.
parent: workspace-sdk
sourceRevision: "962272b3424e266f557fda159231b42c075ed968"
sources:
  - src/domain/workspace/file_browser.rs
  - src/application/directory_observer.rs
  - src/application/workspace.rs
  - src/ports/i_backend.rs
  - src/providers/managed/managed_backend.rs
  - src/supervisor/file_browser.rs
  - src/supervisor/filesystem.rs
  - src/supervisor/dispatcher.rs
  - src/supervisor/protocol.rs
  - tests/workspace_files.rs
  - tests/provider_transports.rs
  - tests/checkpoint_engine.rs
diagramLinks:
  Review: file-sync
diagramDetails:
  - diagram: 0
    node: Reset
    section: Reset and reconnect
  - diagram: 0
    node: Current
    section: Complete reconciliation
  - diagram: 0
    node: Failed
    section: Failed observations
  - diagram: 1
    node: Unknown
    section: Uncertain publication
  - diagram: 1
    node: Review
    section: Existing file editing
---

Applications can browse immediate children, read binary chunks, and poll for changes made by a human, agent, or external tool. The application owns its UI and timer. This first slice creates a complete new file only when the destination is absent. It does not finish the broader human and agent collaborative editing request in [issue 4](https://github.com/nessalabs/workspace-sdk/issues/4).

```mermaid
stateDiagram-v2
    [*] --> Reset: construct observer
    Reset --> Current: complete first poll / replace view
    Current --> Current: complete poll / reconcile upserts and removals
    Reset --> Failed: transport or validation failure
    Current --> Failed: transport or stale continuation
    Failed --> Current: successful complete poll / retain or establish baseline
    Current --> Reset: restart controller / construct new observer
```

## Reset and reconnect

The first successful poll returns a reset. Discard the prior directory view and use its entries. Baselines live only in the observer; no event journal or cursor needs to survive restart. Reconnect to the workspace and construct a new observer for a fresh baseline. Dropping it releases metadata. It owns no background tasks, timers, sockets or watcher handles.

## Complete reconciliation

Each poll reads every page of one directory before advancing its baseline. Results contain new or modified entries and paths now absent. External rename appears as removal plus creation. Empty directories are visible. Short-lived changes between polls are not retained. Observe each expanded directory separately. This is not recursive loading, an event history or an atomic multi-file snapshot.

There is no OS watcher queue to overflow. A future push watcher must reconcile after overflow, disconnect or unknown cursors. This version always reconciles. Stale pagination rejects the poll and preserves its prior baseline.

Pages contain at most 256 entries. Native scans and observer baselines have a 4,096-entry cap; larger directories explicitly return Unsupported. The observer permits at most 16 pages. Request bounds, duplicate paths, direct-child scope, ordering, revisions and cursor progress are checked at boundaries. Names must be UTF-8 portable workspace paths of at most 4,096 bytes; an unrepresentable entry rejects the listing.

Each page rescans metadata. Three local debug-build samples over 4,096 entries took about 1.15–1.20 seconds per poll, or 16 full scans. Poll only visible directories, serialize polls and use a several-second interval with backoff. These are [local samples](../evidence/2026-10-23-workspace-files/files-performance.log), not a statistical baseline or remote latency qualification.

## Failed observations

Transport failure, disappearing entries, unsupported sizes and stale continuations leave the previous baseline intact. Retry a complete poll. Cancellation before completion also preserves that baseline. The next successful poll accounts for current differences, but cannot reconstruct transient history. The SDK installs no hidden retry loop.

## Revision reads and binary files

Versioned reads return at most 256 KiB, a next offset, EOF and a revision. Retain the first token for every following chunk. Discard the buffered content and restart on Conflict. The target checks metadata before and after reading and reopens the pathname to detect replacement. Files may exceed one chunk; no browser-managed full-file allocation occurs.

Tokens fingerprint workspace ID, root, relative path, device, inode, length, mode, mtime and ctime. Keep them on the same connected target. They are metadata observations, not immutable/content-addressed snapshots, authorization or writer locks. Detection depends on filesystem inode/timestamp semantics. Filesystems that hide changes behind identical metadata, remote filesystems and adversarial same-UID namespace manipulation are not qualified. Use immutable checkpoints for stronger snapshot semantics.

## Publication outcomes

```mermaid
stateDiagram-v2
    [*] --> Staged: exclusive create requested
    Staged --> Created: destination absent / link complete anonymous file
    Staged --> Conflict: destination exists / preserve it
    Staged --> Rejected: target rejects before publication
    Staged --> Unknown: acknowledgement lost or durability uncertain
    Unknown --> Inspected: caller reads target before deciding
    [*] --> Review: existing file edit / checkpoint reconciliation
```

Creation stages at most 256 KiB in an anonymous Linux file, syncs it, then exclusively links it into an existing parent. An existing file, symlink, directory or special entry produces Conflict without replacement. Concurrent SDK participants have one winner. A killed staging process leaves no named partial file. Filesystems without anonymous staging return Unsupported.

## Uncertain publication

Unknown means creation may have happened. Read/list and reconcile the destination before deciding to retry. All transport and malformed-response failures after dispatch preserve uncertainty. Only an explicit target rejection or validation before dispatch proves rejection. A parent-directory sync failure also returns Unknown. Cancellation is not rollback. Creation has no durable idempotency receipt and is never automatically replayed.

## Existing file editing

Native revision replacement is Unsupported. This slice adds no delete/rename mutation API; it observes those external changes. The existing low-level chunk write remains non-atomic transfer and must not be used as an editor save operation.

The existing [checkpoint workflow](sync.md) owns three-way detection, preserved base/local/remote conflict sides, explicit resolution acknowledgement, expected local fingerprints and apply recovery. Review does not mutate the destination. Apply requires every writer to stop and an explicit caller quiescence assertion. SDK leases coordinate SDK callers, not arbitrary editors. This is deliberate conflict resolution, not automatic collaborative text or CRDT merging.

The next bounded editing slice should use that owner for managed sessions: capture baseline, stage candidate outside the project, reconcile current contents, acknowledge resolutions and publish while the application fences all participants. The present target abstraction cannot establish that fence. Hash-then-replace is insufficient against unmanaged editors. A provider-native conditional publication primitive or enforceable writer authority is still needed for unattended saves into a live externally editable tree. No duplicate merge subsystem or speculative session API is introduced here.

## Boundaries and provider qualification

Linux operations reuse no-follow descriptor traversal and anchor listing/publication to directory descriptors. Symlinks are visible but never followed. Reads reject nonregular entries such as FIFOs and sockets. The private control root stays separate and ownership-checked. This retains the existing trusted-host permission model; it is not an isolation boundary against arbitrary programs with the same OS user.

The optional IBackend contract and wire operations are provider-neutral. Only Local Linux was exercised end to end. SSH seam regressions use synthetic responses. Boat and container have no new live qualification. They can carry these operations only with a matching deployed supervisor and supported target filesystem. Non-Linux supervisors explicitly return Unsupported. Custom backends default to Unsupported. Existing chunk-transfer capability flags do not advertise conditional publication.

## Evidence and reviewed scope

The sourceRevision names the exact reviewed source commit, based on readiness merge `fa942f883c2888f70bd46a32b53546497ea85133`. [E2E tests](../../tests/workspace_files.rs), [transport regressions](../../tests/provider_transports.rs), [native crash cleanup](../../src/supervisor/file_browser.rs) and [existing checkpoint conflict tests](../../tests/checkpoint_engine.rs) cover different boundaries. Executed gates, review closure and rendered/drill-down validation are recorded in the [review](../reviews/2026-10-23-workspace-files.md). Tests used synthetic workspaces only. No user migration, destructive resync or secrets upload occurred.
