---
id: "open-identity"
title: "Reserve and reconnect workspace identity"
kind: "statechart"
status: "implemented"
summary: "A workspace ID reserves one provider request before target creation begins."
parent: "workspace-sdk"
sourceRevision: "867a4c15e97e025a1066b3c0cd133ffa81f374f1"
sources:
  ["src/application/workspace_sdk.rs", "src/infrastructure/repository_state.rs"]
diagramLinks: {}
diagramDetails:
  [
    {
      "diagram": 0,
      "node": "Saved",
      "section": "Example: readiness failed after creation",
    },
  ]
model: { "spec": "tla/open-identity.tla", "config": "tla/open-identity.cfg" }
modelCheck:
  sourceRevision: "867a4c15e97e025a1066b3c0cd133ffa81f374f1"
  checker: tlc-1.7.4
  date: 2026-11-03T08:45:32.176Z
  config: tla/open-identity.cfg
  modelDigest: cdb3bc237a0bed7a0ddea3e589736f74d70a9a870203eec6781bbc540cb84932
  states: 13
  held:
    - SavedIdentity
    - ReservationStable
  broke: []
  unchecked: []
---

If opening a workspace times out, first determine whether the SDK already saved its target reference. A workspace ID binds one provider request; repeating that request can reconnect instead of creating another target.

## Example: readiness failed after creation

Suppose opening `build-17` returns a readiness timeout. The provider may already have created the target, and the SDK saves a valid descriptor (the saved target reference and capabilities) before checking readiness. Retry the same provider/request under `build-17`; the saved reference selects reconnect. Do not change the request under that ID to force another creation. If the provider call or descriptor save itself failed, the reservation still does not prove whether remote creation happened: inspect provider evidence before deciding. There is no automatic rollback promise.

## Progress and ownership

```mermaid
stateDiagram-v2
    [*] --> Empty
    Empty --> Reserved: persist request fingerprint
    Reserved --> Saved: provider returns matching identity / save descriptor
    Saved --> Ready: readiness succeeds
    Saved --> Saved: readiness fails / retain descriptor
```

`WorkspaceSdk::open` hashes provider plus complete request, reserves it, then reconnects a saved descriptor or opens a provider target. A mismatched returned workspace ID, provider, or version is refused. The descriptor is saved before readiness, so a readiness error does not erase reconnect identity. `connect` rejects unsupported versions and a changed full reference. `list` reads saved descriptors; it does not discover provider resources.

## Model coverage

The model permits two request fingerprints competing for one workspace ID. Reservation and returned descriptor identity are separate guards. It checks that a saved/ready target matches the reservation; it does not model provider idempotency, lost remote replies, hashing, disk durability or readiness liveness. The existing [workspace lifecycle](workspace.md) owns deletion and attached-target policy. Negative controls bypass request reuse and returned identity checks independently.
