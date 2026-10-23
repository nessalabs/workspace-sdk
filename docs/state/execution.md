---
id: managed-execution
title: Run and reconnect to a workload
kind: statechart
status: implemented
summary: The target supervisor owns a durable launch and bounded output after the client disconnects.
parent: workspace-sdk
sources:
  - src/application/run.rs
  - src/supervisor/workload.rs
  - src/supervisor/terminal.rs
  - src/supervisor/terminal_control.rs
  - src/domain/execution/run.rs
  - tests/runtime_pty_adversarial.rs
diagramLinks:
  Stopped: file-sync
diagramDetails:
  - diagram: 0
    node: Lost
    section: Uncertain ownership
  - diagram: 0
    node: Running
    section: Reconnect and control
  - diagram: 0
    node: Stopped
    section: Confirmed stop
---

A run ID binds one command to one workspace. The target supervisor keeps its claim, output, and completion evidence after the SDK handle disappears. Repeating the same launch reconnects; changing its intent is refused.

```mermaid
stateDiagram-v2
    [*] --> Starting: persist launch claim
    Starting --> Running: native process starts
    Starting --> Exited: native launch fails / record exit
    Starting --> Lost: owner evidence unavailable
    Running --> Lost: owner disappears
    state Stopped {
        Exited
        Canceled
        TimedOut
    }
    Running --> Exited: process finishes
    Running --> Canceled: cancellation completes
    Running --> TimedOut: deadline cleanup completes
```

## Reconnect and control

Client wait timeout does not cancel the target process. Retain its workspace and run references to reconnect. Output uses absolute byte cursors and bounded retention. PTY stdout and stderr share one stream. A reported gap means some bytes are unavailable; completion does not imply gap-free output.

Terminal input is transient and limited. A partial acknowledgement permits only the unsent suffix; a missing acknowledgement can mean input was delivered. Unknown delivery must not trigger automatic replay. Resize and control calls have bounded waits. See [PTY integration](../pty-integration.md) for exact budgets and transport contracts.

## Uncertain ownership

Lost means termination could not be established. It is not a stopped state and cannot authorize another writer or handoff. Retain the original launch claim and reconcile the owning target. Do not replace it merely because the client connection failed.

## Confirmed stop

Exit, completed cancellation, and deadline expiry remain distinct recorded outcomes. Cleanup must release owned descendants before completion. Handoff accepts only a confirmed terminal source. Process exit zero still does not establish successful native authentication.
