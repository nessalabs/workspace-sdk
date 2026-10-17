# Boat fixture cleanup: independent quality review

Decision: **FINAL PASS** for source
`c11a41ee26198c677fd38b069ae941cfdd3818a5`.

Reviewer: separate agent `/root/boat_cleanup_quality`, independent of implementation.
The reviewer inspected the actual helper, harness, nine loopback tests, guide,
corrected socket delta, exact cloud checkout SHAs, logs and performance samples.
The review applied Clean Code, SOLID, cohesion, dependency direction and appropriate
concrete lifecycle design. No unresolved P1/P2 or lesser actionable finding remains.

## Scope and findings

The test harness reserves and persists fixture ownership before raw API creation,
records the raw target before SDK bootstrap, then attaches the SDK. A bounded,
cleanup-only entrypoint can resume known targets after failure. Incomplete cleanup
fails the test; an accepted request or missing resource is not completion.

| Finding | Severity | Fix and re-review |
| --- | --- | --- |
| Concurrent ledger controllers could overwrite a newly persisted target with stale state. | P2 | Canonical sibling lock held for the ledger lifetime; a separate controller is rejected before effects. Regression passed on both OSes. |
| Duplicate diagnostic accumulation could eventually prevent saving completion. | P2, separate adversarial review | Fixed classes are deduplicated and bounded. Ten failure/reopen rounds followed by completion pass. |
| macOS accepted test sockets inherited nonblocking mode. | Mechanical failure | Accepted streams explicitly use blocking reads with the existing two-second timeout. Listener shutdown, assertions and 35 ms cleanup budget remain unchanged. |

## Actual validation

[Run 39060249660](https://github.com/nessalabs/workspace-sdk/actions/runs/39060249660)
passed the full `scripts/quality-gate.sh` on Ubuntu 24.04 and macOS 15 for the exact
source SHA. This includes formatting, warning-denying Clippy/docs, both feature
selections' builds/tests, supervisor build and managed/PTY performance checks.
All nine new loopback tests passed once per OS under all features; the no-default
selection correctly runs zero Boat cleanup tests. Both live tests remained ignored.

Each OS produced three managed-run and fifteen PTY samples, with bounded retained
output. These hosted debug samples qualify operation, not a statistical regression
baseline or a speedup. Samples, hashes, failed-run disposition and job metadata are
in [qualification.json](../evidence/boat-fixture-cleanup-2026-10-17/qualification.json).
Run 39059570174 failed on macOS before the socket correction; its Ubuntu job was
cancelled. Neither is passing evidence.

## Limits

This is test-only ownership and cleanup orchestration. Live SDK attachment,
provider deletion completion and production SDK-owned creation/bootstrap durability
remain unqualified. Provider idempotency retention is not established. A terminated
test process can require explicit cleanup resumption. No recovery service behavior
or automatic failover has changed.
