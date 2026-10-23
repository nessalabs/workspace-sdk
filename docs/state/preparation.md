---
id: runtime-preparation
title: Prepare a runtime and authenticate
kind: statechart
status: implemented
summary: Validate resources and credentials, select software, recheck setup, and establish the explicitly chosen authentication method.
parent: workspace-sdk
sources:
  - src/runtime/preparation_service.rs
  - src/runtime/preparation_service/authentication.rs
  - src/runtime/preparation_service/interaction.rs
  - src/runtime/domain/preparation.rs
  - src/runtime/domain/authentication.rs
  - tests/runtime_backend_host.rs
diagramLinks:
  Ready: managed-execution
diagramDetails:
  - diagram: 0
    node: NeedsInput
    section: Missing credentials
  - diagram: 0
    node: Authenticating
    section: Authentication ownership
  - diagram: 0
    node: Ready
    section: Fresh launch plan
---

Preparation binds a complete request to one host scope. It checks resources and credential availability before installing or configuring software. Resuming rechecks actual target state; a saved phase cannot prove that software or credentials remain valid.

```mermaid
stateDiagram-v2
    [*] --> Validating
    Validating --> NeedsInput: credentials unavailable
    NeedsInput --> Validating: caller resolves inputs / retry
    Validating --> Installing: resources and inputs valid
    Installing --> Configuring: compatible executable found
    Installing --> SetupFailed: install cannot establish version
    Configuring --> Authenticating: setup probes pass
    Configuring --> SetupFailed: setup cannot establish state
    Authenticating --> NeedsInput: credential unavailable
    Authenticating --> NeedsAuthentication: native user action required
    NeedsAuthentication --> Authenticating: start or inspect same attempt
    Authenticating --> Ready: native status or selected credentials accepted
    SetupFailed --> Validating: supported retry after correction
    Ready --> Validating: prepare again / recheck target
```

## Missing credentials

The application resolves credential references. Receipts store progress and identifiers, not resolved secret values or native command output. Authentication and billing choices are explicit; subscription authentication never silently falls back to API billing. Transient secret input can still be echoed or printed by the child.

## Authentication ownership

A nonblocking per-preparation lease rejects concurrent controllers promptly. Different IDs remain independent; every controller must share the coordinated store. Login attempts use a stable request, host scope, and attempt counter. Save failure retries the same launch. Only confirmed exit permits a new attempt. Lost ownership requires reconciliation; cancellation failure retains the old attempt.

Native login process exit does not prove authentication succeeded. The native status check owns that decision. A terminal login can be resumed with bounded output, transient input, resize, and cancellation through the exact process reference.

## Fresh launch plan

Only a current successful preparation returns a launch capability. Reusing a saved receipt cannot manufacture one. Software, setup probes, native login state, and freshly resolved credentials are checked again on resume. Real agent sign-in and inference remain separate acceptance work; synthetic process fixtures do not establish those outcomes.
