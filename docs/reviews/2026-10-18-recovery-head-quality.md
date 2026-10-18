# Conditional recovery head: independent quality review

Decision: **PRECLOUD QUALITY PASS**. No open P1/P2 findings in the reviewed generic SDK source. Final acceptance still requires the complete exact-commit cloud mechanical/performance gate and a separate adversarial decision.

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

The head precondition protects recovery authority ordering. It is not a global writer fence, a remote durability guarantee, or an external-effects exactly-once guarantee. Native fixtures qualify process/checkpoint behavior; they do not qualify physical host loss or live remote transport. An unchanged no-default dependency warning remains visible; no all-dependencies zero-warning claim is made. Full cloud mechanical/performance evidence is pending, so this is a precloud code-quality decision.
