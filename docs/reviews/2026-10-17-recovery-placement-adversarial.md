# Recovery placement: independent adversarial review

Decision: **PASS** for source commit
`9da3841e3f26fb13f6f0b224afd7ec65f0b7b1b7`.

Reviewer: separate agent `/root/workspace_handoff_adversarial_review`, independent
of the implementation and quality reviewer. It inspected frozen source and actual
cloud results. No unresolved actionable P1/P2 findings remain.

## Findings, fixes and re-review

| Draft finding | Severity | Final disposition |
| --- | --- | --- |
| Distinct epoch IDs could attach to the same caller-managed project. | P2 | Reject non-SDK-owned descriptors before restore. A real LocalProvider attachment regression checks selected source files stay unchanged, no native claim is created and the pinned unfinished attempt cannot move to a new epoch. Re-review passed. |
| Unsalted authority hashing of transient credentials could provide an offline guessing oracle. | P2 | Authority fingerprints only nonsecret command configuration. Existing native salted claims bind transient inputs when launch is attempted. A historical Started retry returns its accepted reference without authenticating replacement credentials. Executed tests cover identical authority identity with/without transient inputs, no private plaintext in authority, and native refusal of changed credentials after ambiguous launch. Re-review passed. |
| Dropping a caller could release ownership and skip transfer cleanup while work remained in flight. | P2 | Use cases run in owned Tokio tasks that retain the session lease through provider effects and cleanup. Tests gate an actual native open and an actual first archive staging write, abort the caller, require a competing request to conflict, then verify one launch and stable retry identity. The write test verifies multichunk completion and removal of the actual staging file and directory. Re-review passed. |

The cancellation review did not establish an overwrite or duplicate-launch bug in
the original draft: existing identical-import validation and stable native claims
already protect those boundaries. The source-identified draft risk was abandoned
staging and premature effect-ownership release. The final tests target that risk.

The reviewer also checked stale epochs, immutable target/checkpoint identities,
cross-kind repository keys, ordinary-save CAS bypass, concurrent CAS winners,
uncertain launch retry, attachment boundaries and migration. The one-line lint
fix from `1 | 2 | 3` to `1..=3` was separately reviewed as behavior-preserving.

## Actual checks and final decision

[Run 39056115386](https://github.com/nessalabs/workspace-sdk/actions/runs/39056115386)
has the exact reviewed SHA and successful Ubuntu 24.04/macOS 15 jobs. Both executed
the complete `scripts/quality-gate.sh`. Logs confirm two cancellation, seven
repository, eleven recovery workflow and ten existing SDK workflow tests passed
under both feature selections on each OS. Domain transition tests also passed.
Both performance reports match the raw log: three managed-process and fifteen PTY
samples per platform. See
[qualification.json](../evidence/recovery-placement-2026-10-17/qualification.json)
for commands, actual samples, environments, job metadata and log hashes.

The earlier run's Clippy failure is retained as a failed check, not counted as a
pass. Phase write/ack failure injection plus real storage reopen is executed test
evidence. No new production mutation probes or prior-probe reuse is claimed.

With the fixes re-reviewed and actual gates inspected, final adversarial
acceptance is **PASS** for explicit placement and canonical-state retry safety.

## Guarantees deliberately excluded

Canonical epoch checks do not stop an old process or fence arbitrary external
effects. `ExternallyFenced` remains an explicit caller assertion. Providers bound
their own operations; a dropped caller is not a cancellation command. Process or
runtime loss may still leave crash artifacts/staging with no aggregate orphan GC.
Publication intentionally creates a new capture on each call. Hosted performance
samples are qualification, not a statistical baseline or live remote-provider
proof. Automatic failover, offsite durability, physical power-loss recovery and
native agent conversation migration remain outside this accepted increment.
