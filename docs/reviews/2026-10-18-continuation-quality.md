# Acknowledged workspace continuation: independent quality review

Decision: **PRECLOUD QUALITY PASS**. No open P1/P2 findings in the reviewed generic SDK scope. This is not final full-gate acceptance: restored-source local checks, exact-commit cloud quality gate, and separate adversarial acceptance remain required.

Reviewer: `nessa_workload_quality`. Base: `cc57eeb97af706bf5a3d64c34466e797cb9dbdc6`; reviewed uncommitted source identified below. The reviewer read code and actual evidence; did not run builds, tests, or mutation probes.

## Scope and design

`continue_current` reuses the recovery repository, effect lease, CAS transitions and target-local launch claim. The predecessor, workspace origin, exact acknowledged head, nonsecret command identity and new deterministic run are pinned in the existing attempt. Fresh and retained entry points cannot retry one another. Domain validation preserves provenance and command identity across phases.

The application verifies successful predecessor termination and selected canonical content before reservation. Prelaunch retries establish that boundary again. After launch intent, retries never recapture or restore an older baseline over accepted work. The caller must exclude all other writers; these checks are not a global process/filesystem fence. Unknown outcomes remain pinned. No application-specific adapter or provider dependency is added.

## Findings and dispositions

- P2: The original tests did not exercise prelaunch retry revalidation. Closed by before/after-Restored-CAS fault cases, changed-command refusal, and selected-tree divergence after saved intent. The updated eleven-test suite passed; disabling revalidation caused the expected assertion failure.
- P3: A complete canonical bundle remained live across target capture although only its fingerprint was needed. Closed by returning the validated fingerprint from the blocking export closure, releasing bundle bytes before capture.

## Actual evidence inspected

- Updated native continuation integration suite: 11 passed in 20.54 seconds. Cases cover four acknowledged steps in one namespace, distinct run claims, exact retry/reopen, before/after-write failure, stale head/epoch, unsuccessful or uncertain owners, cross-entry-point guards, races, caller cancellation, and prelaunch revalidation.
- Related affected checks: 133 library tests, 2 cancellation tests, 3 replication tests, 7 repository tests and 11 existing recovery workflow tests passed. Includes all three new domain regressions.
- Three semantic mutation probes failed assertions with exit 101 and no compilation errors: selected-tree divergence, prelaunch revalidation, and postlaunch recapture. Source restoration hashes were independently verified.
- Three local macOS aarch64 small-tree samples observed continuation admission at 274.875, 273.522 and 281.433 ms; publication at 186.303, 200.997 and 192.995 ms. These are actual samples, not a statistical regression baseline.

Source hashes (SHA-256):

| File | SHA-256 |
| --- | --- |
| `README.md` | `cf3f4a9aa50671bd80c66f0973074cd7c292cc496faf3a0e09a9cbb06d3dae5e` |
| `src/application/mod.rs` | `668fd1ae394a36e7c42ee2e29a4b926b64912bd1e03f62f9fe30fe883f27d974` |
| `src/application/recovery_service.rs` | `eef201b57b39bca50c16bf8d4a374d36f94ea7faf3a877f183fcde00b44c1132` |
| `src/application/recovery_service/continuation.rs` | `dcac0e9f213a15b73196ff6affff2cc7cc7c9a789a6db451a5c86bc5e427cae1` |
| `src/domain/execution/run.rs` | `87b46757515362132ba413092ff93192acd3820f55a8c9b87ddbe6fcb4e7f2bc` |
| `src/domain/synchronization/mod.rs` | `bcba981d62cfbbfb3e244351d73028cf84f64429cdf4917ed05213629edca9f8` |
| `src/domain/synchronization/recovery.rs` | `8d5559e39d3db3203369fb7795c30683685245dfc99d81839b48a8f5187d11f1` |
| `tests/recovery_continuation.rs` | `27dd064f3b52052bc44797fefc2b72c42086ccd4e979e16c56ca1e70daaef7f8` |
| `tests/support/recovery_continuation_fixture.rs` | `ed4a5add1cdf30d6d169561a3d3e69012e5c056357ed549e425297f369c421b2` |
| `docs/recovery-continuation.md` | `113927e323eccdb4c7b449444893dd89ef7e4d299b36674c4e538c634a0c8461` |

Residual limits: real native fixtures with modeled availability/status faults; no live remote transport or host-death qualification. Selected/excluded paths follow existing checkpoint policy. Verification creates an additional artifact; caller owns retention and durability. No arbitrary external-effects exactly-once claim. An unchanged no-default dependency warning remains visible; passing focused checks does not mean every dependency emitted zero warnings.

Final exact-commit cloud and remaining local results will be appended after they complete.
