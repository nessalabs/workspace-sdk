# Handoff receipt recovery: independent code-quality review

**PASS for the reviewed test-only scope. No unresolved P1/P2 or actionable lesser findings.**

Reviewer: independent `workspace_handoff_quality_review` agent. This is the
design/code-quality half of acceptance; the separate adversarial review remains
an independent requirement. Review round 1; no fixes or re-review required.

## Exact scope and evidence

Reviewed base `0b7bbc0a9be0cce0713df05be56f2a63d6c14c30` on
`codex/workspace-portability`, with the uncommitted change to
`tests/sdk_workflow.rs` only. The source manifest fingerprint is
`ba1b07344214f5098e56d2e2b26e4ff7fd82252e47b0889c546887a5ae030c9b`;
independently checked test-file SHA-256 is
`26b6da60d7365932a5405839aa62a6cfec28f606dd168b49329f4266f8c0d4e6`.
This review record is additional documentation, not a production change.

Read `AGENTS.md`, `coding_standards.md`, the actual test diff, surrounding
workflow tests, `SyncService::handoff`, the file repository's save/lease
implementation, and the native supervisor's launch-claim behavior. No builds,
fault probes, production changes, or sibling PTY changes were made by this
reviewer.

Inspected the parent's evidence rather than re-running it:

- The committed [qualification evidence](../evidence/handoff-recovery-2026-10-17/qualification.json)
  preserves the environment, limits, mechanical result, three managed-run
  samples, 15 PTY samples, and three actual fault failures. Independently parsed
  both performance objects from the raw corrected gate log: each exactly equals
  its corresponding JSON object, including all samples and environment fields.
  The three fault names, tests, exit codes, restoration flags, verification flags,
  and complete panic texts also exactly match the original probe proof. No
  discrepancy found; the scoped PASS above remains unchanged.

- `../workspace-tools-handoff-quality-gate-corrected.log`: the mechanical
  `scripts/quality-gate.sh` completes, including both feature configurations,
  Clippy, formatting, documentation, and local performance qualification.
- `../workspace-handoff-probes-proof.json`: three deliberate production faults
  fail at the intended test assertions: launching before Restored publication,
  changing the destination run identity, and accepting a changed committed
  retry. Each record reports source restoration. These are actual assertion
  failures, not compilation or setup failures.
- `../workspace-tools-handoff-restored-tests.log`: all 10 SDK workflow tests
  pass after restoring production source.

The initial report-path setup failure and cached object-copy warnings remain
part of the parent's evidence. This review makes neither a zero-warning claim
nor a statistical performance claim from the small local samples.

## Design and maintainability assessment

The new fault repository uses the existing narrow `IWorkflowRepository` port
and delegates leases, reads, and unrelated writes to the real disk repository.
It targets one operation and one phase, consumes the injected fault once, and
optionally commits before returning the error. This keeps storage-error
simulation out of production and avoids inventing a parallel storage model.
Capturing the attempted receipt is necessary to observe a run reference that
was returned by native launch but never successfully acknowledged by storage.

The shared scenario helper is cohesive despite its length: setup, injected
boundary, observation, complete controller replacement, conflicting retry,
recovery, and repeated retry form one temporal test. Three descriptively named
entry points preserve individual failure attribution. The two configuration
inputs describe precisely the three supported boundaries, and the explicit
unsupported-case panic prevents silently broadening the helper's meaning.
Further strategy objects, production hooks, or a new fixture hierarchy would
add indirection without improving this scope.

The assertions distinguish the guarantees rather than merely expecting Ok:

- Restored-save failure leaves Captured on disk and no destination launch claim.
- Started-save failure leaves Restored on disk while the real destination run
  completes; commit-then-error leaves Started on disk.
- Reopening creates a fresh repository, SDK, connected workspace handles, and
  SyncService. Recovery does not depend on the fault wrapper's in-memory state.
- A changed continuation is refused before its file effect appears. Recovery
  and subsequent retry return the same destination workspace/run identity and
  leave the incremented destination counter at 5 and source counter at 3.

These observations align with the actual implementation: Restored is published
before native launch; the operation deterministically names the destination
run; native launch persists and checks a claim under its launch lock; and
handoff identity validation precedes the saved Started fast path. The test-only
claim-file inspection is intentional boundary coverage, not an unnecessary
production dependency. The existing workload executable and SDK setup helper
are reused.

## Remaining limits and exclusions

Returned receipt-write errors and committed-write acknowledgement loss are
tested. This is not a process-kill or machine-power-loss experiment. Native
claims and fsync-backed repository behavior are inspected as supporting code,
not treated as proof of every possible filesystem or crash window.

The started destination run is allowed to finish before controller replacement.
This scope proves stopped-source handoff recovery and completed-run identity
reuse; it does not newly prove live-run failover, concurrent controllers,
automatic provider switching, scheduler priorities, remote-provider behavior,
or VM-memory migration. The checkpoint engine is retained across replacement;
the repository and controller are reopened. No broader engine-restart claim is
made.

No new production memory, lock, dependency-direction, or performance burden is
introduced. The small real local fixtures and explicit bounded waits are
appropriate for these regressions. Existing broader synchronization and launch
limitations are unchanged and are not concealed by this acceptance decision.
