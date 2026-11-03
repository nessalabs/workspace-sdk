---
id: "handoff"
title: "Move a stopped workload to another target"
kind: "statechart"
status: "implemented"
summary: "Handoff transfers a checkpoint and starts a continuation only after a
  confirmed source stop."
parent: "workspace-sdk"
sourceRevision: "867a4c15e97e025a1066b3c0cd133ffa81f374f1"
sources: ["src/application/sync/sync_service.rs", "src/domain/execution/run.rs"]
diagramLinks: {}
diagramDetails:
  [
    {
      "diagram": 0,
      "node": "Captured",
      "section": "Example: the laptop stops hearing from the source",
    },
  ]
model: { "spec": "tla/handoff.tla", "config": "tla/handoff.cfg" }
modelCheck:
  sourceRevision: "867a4c15e97e025a1066b3c0cd133ffa81f374f1"
  checker: tlc-1.7.4
  date: 2026-11-03T08:45:31.523Z
  config: tla/handoff.cfg
  modelDigest: 4a65801cb362246f9f277228696a0e1d16d976bf22d48a4dadce87d805830c45
  states: 10
  held:
    - CaptureAfterStop
    - StartAfterStop
    - RestoreAfterStop
  broke: []
  unchecked: []
---

To continue a workload on another machine, first confirm that its source run has stopped. Handoff then transfers selected files and starts the continuation command you supplied.

## Example: the laptop stops hearing from the source

A disconnected client may observe Lost rather than a confirmed terminal run. It cannot use that uncertainty as permission to start the same work elsewhere. Reconnect and establish the source run's terminal state first, stop all other file writers, then hand off to a different empty target using an explicit continuation command. If the handoff caller stops awaiting after admission, the owned operation can still finish. Retain its operation ID and inspect its receipt; observer cancellation does not undo the destination launch.

## Progress and ownership

```mermaid
stateDiagram-v2
    [*] --> New
    New --> Captured: source terminal / persist checkpoint receipt
    Captured --> Restored: source terminal / restore destination
    Restored --> Started: source terminal / start deterministic run
```

The caller must assert quiescence (all other file writers stopped), use different source and destination workspaces, and pass a run belonging to the source. The receipt binds source run, destination, command, secret environment and private input by fingerprint. Reusing an operation ID with different inputs is a conflict. The service rechecks terminal source state before capture, restore and start; Lost is not terminal. A Started receipt returns the recorded result. The destination run ID derives from operation ID, enabling supervisor duplicate-launch protection after a lost receipt save.

Files move, not VM/process memory. The caller supplies the continuation command and must put any portable agent state into allowed files. A failed handoff does not restart the source. Transfer cleanup is best effort; failure to remove transfer scratch does not become a successful cleanup claim.

## Model coverage

Four phases and source states Running, Lost and Stopped check that destination start never accepts uncertain source ownership. The source can become Lost after capture, forcing later checks to matter. Separate invariants correlate capture and restoration to a stopped source. Filesystem contents, identities, provider retries and secret handling remain outside this small model. [Sync](sync.md) owns review and publication.
