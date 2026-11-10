---
id: "persistence"
title: "Keep durable receipts and cooperative ownership"
kind: "contract"
status: "implemented"
summary: "Private repositories retain identities and receipts while cooperative leases serialize participating controllers."
parent: "workspace-sdk"
sourceRevision: "867a4c15e97e025a1066b3c0cd133ffa81f374f1"
sources:
  [
    "src/infrastructure/file_state_repository.rs",
    "src/infrastructure/repository_state.rs",
    "src/infrastructure/local_edit_coordinator.rs",
    "src/runtime/file_preparation_store.rs",
  ]
diagramLinks: {}
---

Private repositories retain identities and receipts while cooperative leases serialize participating controllers.

The file state repository locks before loading state, rejects malformed/unsupported/oversized or symlink state files, and writes through a synced temporary file and directory sync. Version one is interpreted as version two in memory and only rewritten by a successful write. Reads do not silently reset corrupt state. Store data lives outside synchronized projects and can contain sensitive references.

Workspace reservations bind IDs to request fingerprints. Workflow records preserve their typed operation identity. Workflow leases use per-operation nonblocking file locks; another participating controller receives Conflict. Memory stores support the same interface but are process-local. Edit coordination qualifies the lease to a canonical root and store; it does not stop arbitrary editors.

A cancelled async waiter does not imply accepted blocking publication has stopped. Sync closures retain the workflow lease during blocking engine application and receipt save; owned transfer tasks continue through cleanup. Failure after file publication but before receipt save is reconciled through engine apply state, not by blindly replaying a fresh review.

This page describes persistence contracts, not a proof of filesystem durability under power loss or malicious same-UID writers. [Handoff](handoff.md) and [sync](sync.md) own the related workflows. Existing repository unit tests exercise corruption, symlink refusal, reservation persistence and cross-instance lease exclusion.
