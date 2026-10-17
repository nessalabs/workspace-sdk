# Boat fixture cleanup: independent adversarial review

Decision: **FINAL PASS** for source
`c11a41ee26198c677fd38b069ae941cfdd3818a5`.

Reviewer: separate agent `/root/workspace_handoff_adversarial_review`, independent
of implementation and the quality reviewer. Review covered ambiguous creation,
retry identity, failed bootstrap, crash/reopen, concurrent controllers, malformed or
wrong-target deletion receipts, bounded cleanup and diagnostic growth.

## Findings and disposition

| Finding | Severity | Resolution |
| --- | --- | --- |
| A stale concurrent ledger save could erase another controller's new target. | P2 | Nonblocking canonical lifetime file lock excludes the second controller before effects. Separate-controller regression preserves both targets and reopened cleanup. |
| Repeated errors could grow the ledger past its size cap and prevent final completion persistence. | P2 | Fixed error classes use a bounded set; ten failure/reopen cycles followed by successful cleanup and reopen are tested. |
| macOS fake HTTP reads returned WouldBlock. | Mechanical failure | Explicit blocking accepted sockets preserve bounded read timeout and original assertions/budgets. Corrected source rerun passed. |

Both P2 findings are closed. No unresolved actionable findings remain in this scope.

## Evidence checked

The reviewer inspected actual JSON/logs from
[run 39060249660](https://github.com/nessalabs/workspace-sdk/actions/runs/39060249660),
including both exact checkout SHAs, full successful gates and all nine loopback
tests per platform. The no-default selection has zero Boat tests by configuration;
both live entrypoints were ignored. Three managed and fifteen PTY samples per
platform matched the raw logs. These are qualification samples, not a statistical
performance baseline. Full records and log hashes are in
[qualification.json](../evidence/boat-fixture-cleanup-2026-10-17/qualification.json).

The earlier macOS failure and cancelled Ubuntu job remain failed/cancelled evidence.
No production mutation probes or live-provider tests were performed for this gate.

## Accepted scope and residual limits

Acceptance covers durable test ownership before bootstrap, exclusive ledger control,
bounded diagnostics and conservative cleanup accounting. A matching completed
deletion operation is required; acceptance, stop/archive, missing lookup, pending,
blocked or mismatched identity cannot become a false cleanup success.

This does not establish live Boat cleanup success, indefinite server idempotency
retention or production SDK-owned creation/bootstrap persistence. The new harness
precreates raw fixtures and tests SDK attachment. Process loss can require the
cleanup-only entrypoint. Arbitrary external effects are not exactly once, and this
increment does not implement continuous backup or automatic failover.
