# Managed pipe input: adversarial review

**FINAL ADVERSARIAL PASS for `b816ad393d32fe3576a9ceeec0fd953a33cefb95`. No unresolved P1/P2 finding within the managed pipe input/EOF scope.** Separate adversarial reviewer: `workspace_handoff_adversarial_review`; other agents authored implementation/tests. Review did not run builds/probes or change production/tests.

## Source contract

Explicit `start_piped` reuses the existing run claim and detached native owner. A persisted mode flag and piped-only fingerprint tag distinguish it from ordinary/PTY claims, while absent/false mode preserves historical JSON and fingerprint encoding. Launch-time ordinary/private stdin is refused before a piped claim. Optional backend methods default to Unsupported, without a PTY substitute.

One bounded nonblocking OS write acknowledges exactly an accepted prefix, including zero on WouldBlock. It does not acknowledge consumption. Unknown delivery cannot be replayed, and callers must serialize a logical frame's partial suffixes themselves. No input queue/journal is added. Input payloads use private transport and redacted values; echoed output remains potentially sensitive.

The same owner serializes input and EOF, which drops its stdin writer. Repeated close is idempotent; a lost close ACK can be retried without replaying input. Known terminal close first validates genuine piped capability; Lost/missing ownership never establishes Closed. Client framing and waits are bounded, one control request is served per owner iteration, and stdin POLLOUT is excluded from the pipe-mode poll loop to avoid a writable-fd spin. Output, cancellation and deadlines remain serviced.

## Findings and V2 corrections

- **P2 compatibility:** an incidental change rejected empty legacy PTY input. Reverted only that legacy guard; the new managed-pipe nonempty rule remains. Existing one-shot/PTY behavior and hash encoding stay intact.
- **P2 ordering:** terminal status could become visible before a still-held ChildStdin was dropped. V2 explicitly drops stdin after process-group finalization and before terminal publication, making terminal-derived Closed sound.
- **Post-EOF oracle/contract clarification:** the first native roundtrip expected a specific Conflict while owner/socket teardown could race. The actual failing return type was not recorded, so no historical causal diagnosis is claimed. The minimal contract conservatively permits Unknown/unavailable-owner error during teardown, never accepted positive bytes or replay. No later terminal observation reclassifies an earlier post-dispatch Unknown. A persisted EOF marker was deliberately unnecessary for this narrow contract.

The V2 binary test keeps the child alive behind a bounded gate after EOF, making live-owner Conflict independent of teardown. It then releases the gate and compares exact binary output and terminal exit. The ACK-fault case also loses a post-EOF rejection reply, requires Unknown and proves the extra submission never becomes output. Source re-review found both P2 issues closed and no remaining P1/P2 finding.

## Actual evidence so far

The initial run passed 145 library tests and three of four native pipe cases; the roundtrip post-EOF assertion failed. It remains a failed run, not a successful qualification. V2 successfully built the supervisor and passed 161 focused cases: 145 library, four native pipe, two retained byte-output and ten SDK workflow tests. Native tests exercise binary prefix/EOF/reconnect, mode/legacy identity, admission/wrong targets, real pipe backpressure with a stalled owned control client, cancellation and after-dispatch lost/corrupt/malformed acknowledgements without replay. Observation units cover Lost EOF and nonpipe terminal refusal.

Completed parent-owned semantic probes were independently checked against actual failure logs and the combined proof (`pipe-input-probes-combined-proof.json`, retained outside the repository):

- Removing EOF's writer drop produced an actual post-EOF `Accepted { bytes: 9 }` assertion failure.
- Altering legacy fingerprint encoding failed the independently computed historical fingerprint assertion.
- Accepting an oversized acknowledgement produced `Accepted { bytes: 999999 }` instead of Unknown and failed the acknowledgement test.
- Treating Lost as Closed failed the native observation unit's Unknown assertion.

All four tests exited 101 through assertions, rather than compilation failures. Both supervisor-side integration mutants were built successfully before execution. The first oversized-ACK attempt was stopped by the disk guard without a test result; it is retained separately and is not counted as a caught semantic fault. Inactive-cache retirement enabled the completed retry.

All 17 V2 manifest file hashes match after restoration (manifest SHA-256 `0d7cb7a4cda09c837babbadb1b2963d14d2faaa7428c893167b72c5b2b49232a`). A newly built restored supervisor passed before the restored 161-case focused suite: 145 library, four native pipe, two byte-output and ten SDK workflow cases. The restored native pipe group completed in 1.87 seconds; the outer focused command completed successfully in 29.68045 seconds. This is a qualification observation, not a performance comparison.

Actual local all-feature/all-target Clippy with denied warnings, strict documentation and formatting checks also completed successfully. Bounded-run metadata records exit zero, no cleanup error and the owned process group absent for these commands. That runner observation does not itself prove cleanup of detached native workloads; their tests retain separate claim/owner cleanup checks.

**Precloud decision:** no unresolved P1/P2 source or evidence finding. The candidate is ready for an exact-source full cloud gate; this record does not yet accept final shipment or claim remote execution.

## Limits

Closed is endpoint closure, not child consumption, process termination or cleanup confirmation. Canceling an awaiting input future cannot revoke dispatched bytes. Bounded retained output can still gap; this API does not backpressure output producers. No child environment isolation, duplex host adapter, application migration, physical failure, live-provider qualification or arbitrary external-effects exactly-once guarantee is established. Existing native uncertainty, caller cleanup and all-writer contracts continue to apply.

## Final exact-source cloud acceptance

Independently inspected completed run [39168687449](https://github.com/nessalabs/workspace-sdk/actions/runs/39168687449): both Ubuntu 24.04 and macOS 15 jobs and their full `scripts/quality-gate.sh` steps succeeded at exact commit `b816ad393d32fe3576a9ceeec0fd953a33cefb95`. All 17 frozen file hashes match both the working source and that commit's Git blobs. The retained raw cloud log SHA-256 is `49a9e6fa1aae57b13259eca6f278f5ef9947e24a853542df8d366615ad88977a`.

The actual log contains four successful executions of each of the four native pipe tests and two observation units: two feature selections on each OS, totaling 24 new test executions. The full gate additionally completed formatting, denied-warning Clippy, documentation, builds and retained integration checks. Four complete performance JSON objects were parsed from the raw log: three managed samples and 15 PTY samples per OS. These are observed debug-build qualification samples, not a baseline comparison, pipe-specific throughput benchmark or speedup claim.

**Final decision: PASS.** The completed exact-source gate closes the earlier precloud condition. Initial failed qualification and disk-guard-interrupted probe remain retained failures/no-results as described above; later success does not retroactively change them. The source corrections, conservative uncertainty contract and scope limits in this record remain part of acceptance.
