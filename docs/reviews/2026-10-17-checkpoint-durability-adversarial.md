# Checkpoint durability: independent adversarial review

Decision: **FINAL PASS** for
`15e9814c59eb53602e04c8c43ac633c15dfe6021`.

Independent reviewer: `/root/workspace_handoff_adversarial_review`, separate from
implementation and the quality reviewer. Review covered publication ordering,
failure propagation, uncertain retries, constructor interruption, stored corruption,
special files, bounds, source dependency loss and misleading recovery claims.

## Findings and disposition

No unresolved actionable findings remain. The quality review's P2 corruption-test
path error was corrected before execution. The initial runtime failure in the old
Boat fixture was reviewed separately: incomplete requests now return before
recording or executing a fake provider effect. The added regression verifies empty,
partial-header and partial-body abandonment followed by exactly one complete
creation. Larger test-only budgets retain all semantic and request-count checks;
production deadlines remain unchanged. The fix claims no latency guarantee.

Accepted filesystem ordering is files and nested directories, staged root, sibling
rename, then canonical parent ancestry. Post-rename errors preserve visible but
uncertain artifacts. Exact import retry and export readiness verify stored bytes
and the cached tree before syncing and acknowledging. Constructor retries sync
already-visible ancestors. Traversal has explicit entry, byte and depth bounds;
regular-file/directory opens reject symlinks and special final components.

## Executed evidence

The reviewer inspected exact checkout SHAs, full successful gates and individual
tests from [run 39069885745](https://github.com/nessalabs/workspace-sdk/actions/runs/39069885745).
Both Ubuntu 24.04 and macOS 15 pass `scripts/quality-gate.sh`, including formatting,
Clippy, docs, both feature selections, supervisor builds and performance checks.
Eleven durability unit tests and three native independent-store tests pass under
both selections per OS. Ten Boat mock tests pass with all features; live tests are
ignored. Each platform's three managed and fifteen PTY performance samples match
the raw log. These are qualification samples, not a statistical baseline.

[qualification.json](../evidence/checkpoint-durability-2026-10-17/qualification.json)
contains samples, failure disposition, job metadata, hashes and test scope.
Run 39069583255 remains failed/cancelled evidence, never a passing gate.

## Residual limits

The new recovery tests remove source project, artifact and registry directories,
then reopen controllers against independent local artifact and authority stores.
They preserve one native counter increment across lost acknowledgement and exact
retry. They do not kill a physical host or establish power-loss persistence,
off-host storage durability, distributed ownership or arbitrary external-effect
exactly-once execution. The source seed is quiescent.

Canonical ancestry must be readable and syncable through the filesystem root.
Full cached-tree validation and artifact syncing add work under the engine lock.
No checkpoint speedup, active-agent consistent capture, automatic failover or live
Boat qualification is established by this change.
