# Native run inspection ordering: independent quality review

Reviewer: independent quality agent `/root/boat_cleanup_quality`, applying
`AGENTS.md` and `coding_standards.md`. Final reviewed source:
`620ae2faf396fba0214082593a31ec1375e407fc`.

**FINAL PASS for this bounded native inspection ordering change. No unresolved
P1/P2 finding affecting this scope remains.** This decision includes source
review and the completed exact-source full gate, rather than formatting or a
passing badge alone. The separate adversarial review remains independently
required.

## Findings, fixes, and re-review

| Severity | Finding | Disposition |
| --- | --- | --- |
| P2 | An initial Starting/Running read followed by terminal publication and liveness release could return generic Lost from the stale snapshot. | Closed. After observing initial active status and absent owner lock, reread status once. Return a published terminal state or stored Lost unchanged; a still-active receipt remains generic Lost. |
| P2, test fixture | Closing a fixture owner descriptor alone did not establish absent flock ownership in parallel fork-capable unit tests. A repeated absence probe could itself leave a fork-inherited descriptor. | Closed. Explicitly unlock each released fixture owner before dropping it. The genuine abandoned-active case uses one public inspection, retaining Lost/nonterminal and unchanged-receipt assertions. No production liveness handling or assertions were weakened. |

The implementation is seven added lines beyond the old-logic private helper
extraction. It reuses existing bounded, no-follow status reads and the existing
OS lock check. Its responsibility stays within `run_record`: no new public API,
interface, clock, polling loop, claim mutation, retry authority, or sibling
runtime change. Errors on either read or the lock remain errors. Initial
terminal/Lost observations retain the existing fast path. The latest stored
Lost preserves its reason and remains nonterminal under public `RunState`
semantics; `!is_active()` is not used as permission to replay or hand off work.

Source review confirmed publication ordering in the native supervisor:
missing-executable Exited127 is written before owner process exit; ordinary
execution finishes/reaps the process group before final status; PTY execution
shuts down its runtime before publishing final status. This fix relies on that
existing contract rather than establishing stronger fencing or process-death
guarantees. The private helper exposes the actual stale-read boundary for tests
without introducing a parallel implementation.

## Actual evidence checked

I independently read the source/tests, checked the reviewed files against final
HEAD, and compared `docs/evidence/run-inspection-ordering-2026-10-17/qualification.json`
with the saved raw logs and metadata in `../workspace-cloud-run-ID.{log,json}`.
For all three runs, raw-log SHA-256, job metadata, and actual checkout source
matched the evidence record.

- Old-logic red run **39085279935**, source
  `e1e6a1461b0bf7e584218f80c5daa492c982e994`: both OS jobs failed at the initial
  all-feature unit gate, with exactly two expected new-test failures and three
  new safety-test passes. Actual assertions showed Lost instead of Exited0 and
  failure to propagate a missing-status reread error. The terminal matrix stops
  at its first Exited0 case; the error matrix stops at missing status. Other
  matrix branches were not demonstrated red.
- First fix validation **39085468861**, source
  `1180d0c6396ebbcdb90774137266e0d03ba2ffdd`: both OS jobs failed. The terminal
  matrix observed Starting instead of published TimedOut; Ubuntu also failed
  the genuine-Lost case. Source behavior shows that the owner-live branch was
  taken, so close-only fixture release did not establish the intended
  precondition. Fork-inherited flock descriptors are a source-supported
  explanation, not an instrumented identification of a particular child.
- Corrected-fixture full run **39085792132**, final source above: Ubuntu 24.04
  and macOS 15 completed successfully. All six new test names passed twice per
  OS, across all-features and no-default-features: **24 new test executions**.
  The retained missing-executable runtime test passed twice per OS. The actual
  full script completed formatting, warning-denying Clippy, builds, both test
  selections, warning-denying documentation, and both ignored performance
  qualifications.

The first five tests preceded old-logic extraction; the sixth stored-Lost case
was drafted after the red run and before the fix. The explicit-unlock fixture
correction followed the failed fix validation. The corrected fixture was not
rerun against old logic; no such red result is claimed. Both failed runs remain
preserved evidence, not passing qualification.

The final completed tests cover initial Starting/Running crossed with
Exited0/Exited127/Canceled/TimedOut, held-owner active behavior, abandoned-active
uncertainty, terminal/Lost fast paths despite an invalid lock entry, preservation
of a newly published Lost reason, and missing/malformed second-read errors.
They use real disposable files, atomic status publication, and OS locks without
sleep-based race scheduling. Explicit unlock is fixture choreography; native
production owner release remains unchanged.

## Performance evidence and limits

I independently extracted both JSON performance objects from each OS raw log
and compared the complete objects with qualification evidence: **3 managed and
15 PTY samples per OS**, including environment and all sample fields. Managed
cancel observations were 52.47–52.69 ms on Ubuntu and 151.92–218.50 ms on macOS;
PTY cancel observations were 52.57–58.23 ms and 56.21–216.73 ms respectively.
These are hosted debug runtime qualification samples, not an inspection
benchmark, speedup claim, or statistical regression baseline.

The controlled old-logic failure proves a possible inspection failure mode. It
does not prove the cause of the earlier Ubuntu 39083477158 missing-executable
failure recorded in scanner qualification; that evidence remains preserved.
The conditional reread does not repair corrupt state or guarantee outcomes
after arbitrary filesystem or lock failures. No live Boat/SSH, custom-backend
exactly-once, deployment, power-loss, or external-effects guarantee follows
from this review. No local builds/tests or source edits were performed by this
reviewer.
