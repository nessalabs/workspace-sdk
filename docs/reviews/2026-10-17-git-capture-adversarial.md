# Source Git discovery: adversarial acceptance

Decision: **FINAL PASS** for exact source `db6e2fa7141503d6f91c079759a1c647f3f2afba`. No unresolved P1/P2 finding affects this scope.

Reviewer: adversarial subagent, separate from production implementation and code-quality review. Disclosure: this reviewer identified the issue and authored the first four integration regressions before implementation. The parent authored the fifth direct target-export case and strengthened nested positive assertions; the implementation owner added two marker unit tests. This is independent production-source review, not independent authorship of every test.

## Finding and fix

**P2, closed:** source `tracked()` previously treated every failed `rev-parse --is-inside-work-tree` probe as a non-Git directory. A linked worktree's `.git` file points to shared metadata elsewhere. When that metadata becomes unavailable, returning an empty tracked set lets ignore rules omit tracked files while capture or export acknowledges success. This was a source-identified risk; no executed old-source failing test or mutation probe is claimed.

Reviewed the actual `capture.rs` change, guide, fixture and test code. Successful exact `true` continues to inspect the real index and propagates its failures. Unexpected successful output refuses explicitly. A failed or exact `false` probe checks the capture root and ancestors for any `.git` entry using no-follow metadata. Existing files, directories and dangling symlinks count as evidence; errors other than `NotFound` propagate. Observed Git context therefore fails closed instead of changing tracked-file selection. No-marker directories retain their existing fallback behavior.

The fixture creates a real linked worktree and proves that committed, force-added files also match ignore rules. Healthy capture and archive export retain root/nested tracked files while omitting ordinary ignored data and Git metadata. Broken cases rename only disposable shared Git metadata, leave the linked pointer unchanged, and require capture/export refusal. A subdirectory case requires ancestor recognition. The target-export case invokes the real filesystem helper and verifies no archive was published. Source contents, pointer and all regular shared Git metadata are compared against saved bytes. No source-repository reconstruction or mutation is introduced.

No additional actionable P1/P2 issue remained on final source review. The implementation is deliberately conservative; it does not parse Git pointers or invent a repository registry, and the guide states its quiescence and alias limitations.

## Verified actual evidence

Inspected [cloud run 39081716244](https://github.com/nessalabs/workspace-sdk/actions/runs/39081716244), its downloaded job metadata and raw logs, the exact checked-out source, and the [qualification evidence](../evidence/git-capture-discovery-2026-10-17/qualification.json). Both Ubuntu 24.04 and macOS 15 checked out the exact source and successfully completed `scripts/quality-gate.sh`.

The actual gate passed formatting, warning-denying all-target/all-feature Clippy, both feature selections, examples, warning-denying docs and managed/PTY performance qualification. Independently counted each of the five integration cases and two marker unit tests passing twice per OS: **28 new-test executions**. Existing non-Git checkpoint and retained suites ran in the full gate. No builds/tests were rerun by this reviewer during acceptance.

Verified qualification metadata against the actual run and raw-log SHA256 `e3000cdddd9d438c8199e5ea4f5f91c72589e2d4611bf0cf57cd14573f5faf62`. The evidence contains complete three managed and fifteen PTY sample objects per OS, matching raw log objects exactly. These are hosted debug-build Linux x86_64 and macOS aarch64 runtime samples, not a statistical regression baseline or a capture speedup benchmark.

## Limits

- Keep source files and Git metadata quiescent. The probe and subsequent marker checks do not atomically bind repository identity or prevent concurrent disappearance/replacement.
- Recognition is conservative. An ancestor marker may cause refusal across a Git discovery or mount boundary. Absence of a marker does not classify every possible Git failure. Bare repositories without a `.git` marker retain the prior behavior; this is not a bare-repository backup feature.
- Unexpected-success output and non-`NotFound` metadata I/O branches were source-reviewed, not separately runtime fault-injected. The dangling-marker unit test qualifies marker detection, not a separately injected full capture operation.
- Direct `export_remote_tree` coverage exercises the actual local target-side filesystem helper. It is not provider transport, live SSH/Boat, offsite replication or physical power-loss qualification.
- Checkpoints retain selected files, not Git history, branches, indexes or a reconstructable worktree association. Return/apply qualification for an existing linked checkout is a separate boundary, not established by these tests.
- Existing recovery, handoff and uncertain-run policies remain unchanged. No new scheduler, coordinator, identity framework, automatic failover or billable resource is introduced.
