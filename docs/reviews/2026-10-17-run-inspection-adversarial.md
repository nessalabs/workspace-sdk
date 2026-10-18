# Native run inspection ordering: adversarial acceptance

Decision: **FINAL PASS** for exact source `620ae2faf396fba0214082593a31ec1375e407fc`. No unresolved P1/P2 finding affects the final increment. This establishes the controlled status/liveness ordering defect and its fix; it does not prove the cause of the earlier scanner-qualification runtime failure.

Reviewer: adversarial subagent, separate from production implementation and code-quality review. Disclosure: this reviewer identified the suspected ordering issue and authored the tests before the corresponding production changes. Production code received independent source review; test authorship is not presented as independent test design. The first five tests preceded old-logic extraction. The sixth persisted-Lost case followed actual RED evidence but preceded the fix. This reviewer also applied the later fixture-only correction after independent review.

## Findings, fix and fixture correction

**P2, closed: stale active status could become false Lost.** Inspection originally read `status.json`, then inspected liveness. A supervisor can publish terminal status and release liveness between these observations. Declaring Lost from the stale active snapshot then ignores an already published stopped result.

The production fix performs one additional validated status read only when the first status was active and the liveness lock is absent. A freshly recorded nonactive state is returned unchanged, including a stored Lost reason. If the fresh status remains active, the existing generic Lost result remains. Missing/malformed second reads propagate errors. There is no production polling, automatic retry, receipt rewrite, new run identity or invented stopped evidence.

Reviewed native workload ordering: descendants are finalized and the retained leader reaped before terminal publication; the detached supervisor retains liveness through its return/exit. `Lost` remains nonterminal. Initially observed terminal/Lost states do not require a liveness query; a genuinely held owner keeps an active observation active.

**P2 fixture assumption, closed:** the first fixed-code run exposed that dropping the fixture owner descriptor did not reliably establish absent liveness during concurrent unit tests. The observed original Starting result is consistent with the helper correctly seeing a still-held lock. Forked test children can inherit a descriptor sharing its flock until exec/exit; this is a source-supported explanation, not an instrumented attribution of each failed observation. The fixture now explicitly calls `LOCK_UN` through `fs2::FileExt::unlock` before dropping its owned descriptor. That releases the shared flock across inherited duplicates. The genuine-Lost test uses public inspection once, avoiding an additional lock query whose probe descriptor could itself be inherited. No raw PID signal, global test serialization, new decision seam or production locking change was added. Explicit unlock is fixture choreography; native production release remains unchanged.

The unchanged post-status helper remains the exact production decision boundary. Tests force real file/OS-lock ordering without sleeps or a probabilistic race. Final assertions preserve terminal exit codes, cancellation/timeout, genuine Lost uncertainty, stored Lost reasons, and missing/corrupt status errors. No additional actionable P1/P2 issue remained after final source re-review.

## Actual RED, failed validation and successful acceptance

All three runs and their limitations are retained in the [qualification evidence](../evidence/run-inspection-ordering-2026-10-17/qualification.json). Independently checked actual source SHAs, job metadata, raw logs, named test results and hashes.

1. [RED run 39085279935](https://github.com/nessalabs/workspace-sdk/actions/runs/39085279935), source `e1e6a1461b0bf7e584218f80c5daa492c982e994`: both OS jobs failed the two intended regressions against extracted old logic; the other three safety tests passed. The first terminal assertion returned Lost instead of Exited0. The terminal matrix stopped at that first assertion; this RED did not execute every matrix combination, the later sixth test, or both feature selections. It used the original close-only fixture.
2. [First fix validation 39085468861](https://github.com/nessalabs/workspace-sdk/actions/runs/39085468861), source `1180d0c6396ebbcdb90774137266e0d03ba2ffdd`: both OS jobs failed the terminal matrix with Starting instead of TimedOut; Ubuntu also failed the genuine-Lost case. The other cases passed. This was not accepted or suppressed; it prompted the fixture correction above. Production fix code remained unchanged.
3. [Final full run 39085792132](https://github.com/nessalabs/workspace-sdk/actions/runs/39085792132), source `620ae2faf396fba0214082593a31ec1375e407fc`: both Ubuntu 24.04 and macOS 15 checked out the exact final source and successfully completed `scripts/quality-gate.sh`. All six new named tests passed under both feature selections on both OSs: **24 new-test executions**. The completed terminal test exercises Starting/Running crossed with Exited0, Exited127, Canceled and TimedOut. The retained missing-executable runtime integration case also passed twice per OS.

The final gate includes formatting, warning-denying all-target/all-feature Clippy, both feature selections, examples, warning-denying docs and managed/PTY qualification. Complete three managed and fifteen PTY sample objects per OS match the raw logs exactly. These hosted debug-build Linux x86_64 and macOS aarch64 runtime samples are not a statistical regression baseline or inspection-latency benchmark.

Verified raw-log SHA256 values:

- RED: `5fce1fd80ab42956e7cbd5d64398c204d0241c41b67ec69769fbb556eb96a82a`.
- First fixed-code failure: `a5e76b8c09ca36a407124aeb961bca4ca8232fcf8b6efa4c6cb90347175b7ff6`.
- Final success: `9f587bcfd976b56ea4df574cf8a4661983c0ace700da307b6466163a326c67d4`.

No evidence discrepancy was found. This reviewer performed no builds/tests during acceptance and edited only this review record.

## Limits

- The corrected explicit-unlock fixture was not rerun against old logic. Earlier RED and final GREEN use different release choreography; no identical-final-test RED/GREEN claim is made.
- A private helper accepts the first actual status observation so tests can force publication before the actual liveness query. This qualifies that production decision sequence, not a complete independently scheduled supervisor crash. Public inspection assertions cover its delegation and ordinary safety paths.
- The earlier scanner run's missing-executable failure remains historical evidence. Matching error text and this controlled defect do not establish its exact runtime interleaving or exclude other causes.
- The fix assumes existing immutable run identity and supervisor-owned status/liveness files. It is not protection against malicious external file replacement, a new cross-host authority protocol, offsite durability or hardware power-loss qualification.
- Genuine missing liveness with active status still reports Lost and cannot authorize handoff as stopped. No replay, failover, cancellation or deletion policy was relaxed.
