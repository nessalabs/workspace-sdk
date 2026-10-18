# Acknowledged workspace continuation: independent quality review

Decision: **FINAL QUALITY PASS** for source commit `81b843496d8f13566ea9d41e6b233d513ac19b31`. No open P1/P2 findings. The independent quality decision incorporates actual restored-source local checks and the complete exact-commit Ubuntu/macOS cloud gate. The separate adversarial decision remains independently required for aggregate acceptance.

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

## Final exact-source verification

The reviewer independently hashed all ten source files above and compared each with both the current tree and `git show 81b843496d8f13566ea9d41e6b233d513ac19b31:<path>`; every hash matched. Source-manifest SHA-256: `8aa62ad2cb558f76967e1791bae5eefa03a6a74e1a7129096020a1919ea2bd81`.

After all mutation probes were restored, the eleven native continuation tests passed again in 19.82 seconds. Local all-feature/all-target Clippy with `-D warnings` passed in 9.20 seconds; strict all-feature documentation passed in 4.07 seconds; formatting passed.

[GitHub Actions run 39150579591](https://github.com/nessalabs/workspace-sdk/actions/runs/39150579591) completed successfully for the exact source commit on Ubuntu 24.04 and macOS 15. Both jobs ran the complete `scripts/quality-gate.sh`: formatting, all-feature/all-target Clippy, both example feature builds, all-feature tests, SPACE-only contract tests, no-default tests, strict documentation, supervisor build and both explicit performance qualifications. Both raw logs contain the mechanical-completion marker.

The new continuation suite passed in both all-feature and no-default configurations on both platforms: 44 case executions, zero failures. Suite times were 3.39/3.43 seconds on Ubuntu and 6.61/7.50 seconds on macOS. The raw cloud log also contains three managed-runtime samples and fifteen PTY samples per platform, covering output-load reconnect/read/cancel and PTY interaction/resize/cancel scenarios. These measured samples do not establish a statistical regression baseline.

The reviewer inspected the complete run metadata and raw test/performance output. SHA-256 of run metadata: `9bdf6c8b850b124645e6db5446977ff522fae4e02ab831efa77154e92571291f`; raw log: `f67c30aca03cef0d2245df3d403829d1f252c4a8426495558d498d751e4580b3`.

No production/test source changed during this final review. Subsequent review/qualification prose may be committed separately without claiming that cloud tested that later documentation commit. All findings above remain closed. Final independent quality decision: **PASS**.

The final evidence-only delta was also reviewed: `docs/evidence/recovery-continuation-2026-10-18/qualification.json` correctly binds the source hashes, exact successful cloud run, 44 continuation case executions and measured limits. Every object in `performance.json` was independently re-extracted from the raw cloud log and compared for exact JSON equality: four reports, with 3 managed and 15 PTY samples per OS. No application-private material is included. The evidence-only delta introduces no new source behavior or unresolved quality finding.
