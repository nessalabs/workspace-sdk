---
id: "feature-map"
title: "Feature ownership and verification map"
kind: "guide"
status: "implemented"
summary: "This map links every public feature area to its source owner and states the verification boundary."
parent: "workspace-sdk"
sourceRevision: "867a4c15e97e025a1066b3c0cd133ffa81f374f1"
sources: ["src/lib.rs", "src/ports/i_backend.rs", "src/runtime/mod.rs"]
diagramLinks: {}
---

This map links every public feature area to its source owner and states the verification boundary.

| Feature                                               | Source ownership                                           | Documentation and model disposition                                                          |
| ----------------------------------------------------- | ---------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| Provider registration, open, reconnect, list          | application/workspace_sdk and builder                      | [Workspace](workspace.md), [open identity model](open-identity.md)                           |
| Local, SSH, container, Boat; deployment and ports     | providers and IBackend                                     | [Provider contracts](providers.md), [workspace](workspace.md); external transports unmodeled |
| Start, reconnect, inspect, wait, cancellation         | application/workspace and run; supervisor/workload         | [Execution](execution.md); source-documented, no new process model                           |
| PTY input, resize, output cursors and gaps            | supervisor/terminal and terminal_control                   | [Execution](execution.md); pinned pty-runtime is an external implementation                  |
| Directory paging, observer, versioned reads, create   | application/directory_observer; supervisor/file_browser    | [Files](files.md), [baseline model](directory-baseline.md)                                   |
| Capture, archive validation, three-way reconciliation | checkpoint/infrastructure                                  | [Sync](sync.md); engine internals source-documented, not formally modeled                    |
| Apply preflight, conflicts, recovery and rollback     | checkpoint/infrastructure/apply, inspect_apply             | [Sync](sync.md)                                                                             |
| Initialize, return sync, baseline advancement         | application/sync                                           | [Sync](sync.md); transfer/hash details not formally modeled                                  |
| Stopped-source handoff and continuation               | application/sync/sync_service                              | [Handoff model](handoff.md)                                                                  |
| Resource/tool checks, setup recipes, credentials      | runtime/preparation_service and policies                   | [Preparation](preparation.md); provider login/live installs not run                          |
| Interactive native authentication and input           | runtime/preparation_service/authentication and interaction | [Preparation](preparation.md), [PTY integration](../pty-integration.md)                      |
| State repositories, receipts, workflow/edit leases    | infrastructure and runtime stores                          | [Persistence](persistence.md); filesystem durability not formally modeled                    |
| Identifiers, paths, references, bounds, errors        | domain and checkpoint/domain                               | Contracts in owning pages; immutable values do not receive invented lifecycles               |
| Supervisor CLI/protocol and distribution              | bin, supervisor/cli, release scripts                       | [Architecture](../architecture.md), [release](../release.md)                                 |
| Examples and qualification                            | examples and tests                                         | [Verification](verification.md), [new evidence](verification-current.md)                     |

New pages record the exact inspected source. Existing broad pages retain their earlier checkpoints and historical evidence; this addition does not claim a complete re-audit of each line of those pages. Four focused models cover the listed critical boundaries, not every feature implementation. The atlas includes all feature areas while distinguishing contracts, external dependencies and unmodeled lifecycle detail.
