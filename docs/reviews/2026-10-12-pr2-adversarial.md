# PR 2 follow-up adversarial review

Reviewer: independent adversarial agent `retained_eof_adversarial`.

Scope: initial `pty_live_probe` state-save failure and durable, reproducible
sanitized evidence for the existing Vercel Linux qualification. This review
does not rerun or expand the remote provider qualification.

## Findings and disposition

1. **P2 — opened workspace leaks when the first recovery-state save fails.**
   Independently confirmed the public `start` path calls `provider.open`, then
   returns from `save` without retaining a cleanup path. No durable descriptor
   exists from which the separate cleanup command could reconnect. Added
   `tests/pty_live_probe.rs`: a public executable subprocess ignores SIGXFSZ and
   sets RLIMIT_FSIZE to 1024 bytes, allowing compact ownership/deletion metadata
   while failing the larger initial state write. It requires the original
   filesystem error to remain visible, no persisted state, a completed owned
   deletion receipt, and independently absent owned directories. No production
   failure hook is used. RED: `work/live-vercel/pr2-rollback-red.log`,
   0/1 in 0.81s, actual EFBIG from the initial temporary state file followed
   by missing deletion receipt. The fix retains the opened backend, attempts
   destruction with a 30-second timeout, reports all incomplete cleanup
   outcomes, and returns the original save error. Executable resolution now
   occurs before ownership acquisition. GREEN:
   `work/live-vercel/pr2-rollback-green.log`, 1/1 in 0.69s. Independent source
   re-review and boundary regression **close this P2**. The quality-gate script
   builds the example under each feature configuration before its regression
   test, so a stale or absent example cannot silently substitute for the change.
2. **P2 — private ignored logs cannot support a clean-checkout audit.**
   The previous review referenced `work/live-vercel`, whose raw artifacts are
   not committed. Acceptance requires durable sanitized receipts/logs and
   reproducible exact command templates, preserving failed and successful
   outcomes, the capability qualification, source provenance, and independent
   remote-controller boundaries. The committed
   [`vercel-linux-pty-artifacts`](../evidence/vercel-linux-pty-artifacts/README.md)
   now supplies 30 original/sanitized hash pairs, full gate logs, separate RPC
   receipts, both performance samples, environment/source manifest, historical
   probe source, initial failure, recovery failure, and stop plus stopped-state
   receipts. Independently matched all 27 original digests to private originals
   (historical source to commit `3e19915d4453a76471e3b7106515c088ac934bce`),
   verified all published digests, and compared bytes after only the declared
   consistent identifier/hostname aliases. The three additional local RED/GREEN/final-gate logs also match their
   original/published digests and are byte-identical after only the declared
   checkout/home path substitutions. No other differences occurred; all
   JSON parses and numerical performance/source files are unchanged. README
   gives the exact capability-dropping and separate-controller command lines;
   RPC records preserve the historical argument arrays and actual outcomes.
   The operator transcription remains explicitly distinguished from fetched
   raw evidence. **Closed after independent artifact/provenance re-review.**

## Decision

**PASS** for the PR2 follow-up scope. Both P2 findings are closed; no remaining
P1/P2 was found. The independent reviewer inspected the completed
`scripts/quality-gate.sh` output, now committed as
[`pr2-final-gate.log`](../evidence/vercel-linux-pty-artifacts/pr2-final-gate.log)
(published SHA-256 `3d113b10b3f1fb8c142fd4e74477ec246fc7574ad769441259d542056eec65ff`;
original SHA-256 `a83148d150c00cb0de319cc00b665497ea2ef79f3311fac165be14b7fc934593`).
The committed [`pr2-tested-source.json`](../evidence/vercel-linux-pty-artifacts/pr2-tested-source.json)
independently matches all three current changed source files and the published
gate log. Relative public artifact links checked in both evidence READMEs and
this ledger resolve.
Formatting, strict Clippy, both feature configurations, rustdoc, supervisor and
probe builds, and both performance qualifications passed. The new real boundary
regression passed under both configurations (0.80s and 0.33s); native/adversarial
PTY tests remained 4/4 and 16/16 in both. Pipe qualification took 1.34s and PTY
qualification took 15.78s with 15 actual samples and 131072 retained bytes each.

Reviewed final source hashes:

- `examples/pty_live_probe.rs`: `fa125f687d51271f9ee3879c326ab5cf650eea2f9fb14409efa57701b58eb99b`.
- `tests/pty_live_probe.rs`: `6d4b7a2aca924f7480c3977ca1ca348c6487540c0ef9c7c810d5da02eb21a103`.
- `scripts/quality-gate.sh`: `9f9c85e3a0ce9ba745753ae21233fb25d81e4f2bd0be6ac7f9ee34220b02b53b`.

The initial-save regression exercises EFBIG, not every possible disk/fsync/persist
failure. Destruction failure or timeout remains reported alongside the original
save error; this patch cannot guarantee cleanup when the underlying filesystem
or supervisor is unavailable. The later probe fix was verified locally and must
not inherit the historical remote source hash. The remote evidence still qualifies
only capability-constrained LocalProvider execution on Vercel Linux, with the
provider/transport exclusions in its README. Hosted CI and merge remain the
parent task's separate gates.
