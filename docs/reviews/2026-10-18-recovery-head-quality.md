# Conditional recovery head: independent quality review

Decision: **FINAL QUALITY PASS** for source commit `ed3ac4ef29c26cf20f73a9ad26192288799857f2`. No open P1/P2 findings. The exact-commit full cloud mechanical/performance gate has passed on both supported CI platforms; a separate adversarial decision remains independently required for aggregate acceptance.

Reviewer: `nessa_workload_quality`. Base: `5f1bb35d2191143913ece0c4228a16d490e64135`; uncommitted changed source is pinned below. The reviewer read source, actual logs and probe failures and verified hashes; ran no builds, tests or probes.

## Scope and design

`RecoveryService::recover_at_head` adds one optional precondition to the existing owned recovery workflow. A new attempt compares the exact canonical head after acquisition of the recovery effect lease and before preparation or provider effects. An exact retry compares its original pinned checkpoint rather than a later published head. A same-epoch publication therefore cannot silently substitute a different checkpoint.

The existing `recover` entry point passes no head precondition and retains its semantics. Both methods use the same owned Tokio task, repository effect lease, revision CAS, phase transitions, native launch claim, uncertainty handling and fresh-versus-retained retry guard. No new journal, credentials field or parallel transaction abstraction is introduced. The source change is cohesive and depends on existing domain values.

## Actual evidence and findings

- The affected suite passed 169 cases: 133 library, 2 cancellation, 11 continuation, 2 expected-head, 3 replication, 7 repository and 11 workflow tests.
- The final restored expected-head suite passed both real native cases in 2.73 seconds. It proves stale same-epoch head refusal without target/claim/authority mutation; correct-head placement; retry after publication using the original checkpoint; changed retry-head refusal; and a forced publisher winning before effect-lease acquisition.
- All-feature/all-target Clippy with `-D warnings` passed in 3.91 seconds, strict all-feature documentation passed, and formatting passed.
- Three semantic mutations produced assertion failures with exit 101 and no compilation errors: omit the head check, compare a retry with the current head, and move the head check before effect-lease acquisition. The touched production source was restored exactly before final checks.
- Initial test compilation failed because a test assumed `RecoveryRequest: Clone`; it now constructs the same request on each call. Clippy then exposed unused helpers in the reused shared test fixture. The final narrow `dead_code` allowance applies only to that imported test module; no production lint policy changed. Both failures remain historical failure evidence.

No unresolved correctness or code-quality finding remains. The same-epoch publication race identified during design is addressed by the effect-lease precondition and the actual forced ordering test.

Source-manifest SHA-256: `936f03656637380c9a09330b8ff1485b89c63a47ed55f79bf04bd1304f52d6aa`. All four current source hashes were independently verified.

| File | SHA-256 |
| --- | --- |
| `README.md` | `5adfa69050154ef5e04a4db53c8a73384c0751c9507eb70bd518c85ce1aaf943` |
| `docs/recovery-expected-head.md` | `85520af8b02bdb9a01649adc8f12918e9da0bce6ddcd5071154fd73c90855163` |
| `src/application/recovery_service.rs` | `e6b392b405b816e00136949342faa60bef3bf5c4ff49e0e880134e57545c56aa` |
| `tests/recovery_expected_head.rs` | `4d6c49abad94d942226d64abf9f2d5e57513ef09c05d5a64b622f2f1113e7a9e` |

## Residual limits

The head precondition protects recovery authority ordering. It is not a global writer fence, a remote durability guarantee, or an external-effects exactly-once guarantee. Native fixtures qualify process/checkpoint behavior; they do not qualify physical host loss or live remote transport. An unchanged no-default dependency warning remains visible; no all-dependencies zero-warning claim is made. Final cloud evidence below completes this independent quality decision.

## Final exact-commit cloud evidence

[GitHub Actions run 39157524086](https://github.com/nessalabs/workspace-sdk/actions/runs/39157524086) completed successfully for exact commit `ed3ac4ef29c26cf20f73a9ad26192288799857f2` on Ubuntu 24.04 and macOS 15. Both jobs completed the entire `scripts/quality-gate.sh`, including formatting, all-feature/all-target Clippy, both feature-mode example builds, all-feature tests, SPACE-only contracts, no-default tests, strict documentation, supervisor build and both explicit native performance qualifications. Both raw job logs contain the final mechanical-completion marker.

The expected-head suite passed twice per OS: all-feature and no-default configurations, eight new case executions with zero failures. Suite times were 0.35/0.36 seconds on Ubuntu and 0.94/0.94 seconds on macOS. The reviewer independently extracted all four performance JSON objects from the raw log and compared every field for exact equality with `docs/evidence/recovery-expected-head-2026-10-18/performance.json`: three managed-runtime samples and fifteen PTY samples per OS. These are actual runtime/output-load/reconnect/read/cancel and PTY interaction/resize/cancel samples, not a statistical regression baseline.

All four frozen source hashes above were independently compared with both the current tree and exact candidate Git blobs; every hash matched. The final qualification-only delta correctly binds the same source hashes, run, limits and eight new executions. No production or test source changed during final review, and later evidence/review prose must not be described as a separate cloud-tested commit.

Raw cloud log SHA-256: `fe8e8c8c6857b3bac839675111051610ab2f2f8e177207342e64180fca46621b`. Run metadata SHA-256: `c60fb8d0f7736c679913002fcff36811a6f1b85248c4c3cb9828be7191afa33b`. Both were independently verified against the qualification record. The reviewer did not rerun builds or tests.

Final independent quality decision: **PASS**. No unresolved P1/P2 findings.
