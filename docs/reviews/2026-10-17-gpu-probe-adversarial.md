# GPU probe collection: adversarial acceptance

Decision: **FINAL PASS** for exact source `3224bb1d60f34041b571938dbfb6c66dbccb556f`, following successful complete revalidation with prior failed runs retained. No unresolved P1/P2 finding affects this increment. The separately observed PTY resize failure remains unexplained; this acceptance does not claim its cause was fixed.

Reviewer: adversarial subagent, separate from production implementation and code-quality review. Disclosure: this reviewer identified the collection risks and authored the five test cases before the corresponding production changes. The code-quality reviewer identified an elapsed-time oracle gap, closed before actual RED execution. Production source then received independent review. Test authorship is not presented as independent authorship of every qualification artifact.

## Findings and disposition

**P2, closed: child-exit polling did not bound subsequent stdout reads.** Old code waited for success, then synchronously read stdout. A remaining writer could keep the stream open indefinitely after the direct child exited. Actual RED exercised an exited child plus an externally held stream writer: collection waited about the two-second fixture fail-safe despite its 100 ms budget.

**P2, closed: waiting before draining could stall a valid producer.** A producer could block on output capacity before exiting, then be treated as unknown at the deadline. A real Unix stream fixture constrains and reads its actual send-buffer size; 4000 valid CSV rows exceed that buffer while remaining below the output cap. Old code returned zero devices instead of 4000 on both OSs.

**P2 test oracle gap, closed before RED:** empty output alone could pass if collection blocked until the writer fail-safe and only then noticed expiration. The held-writer test measures around collection, before cleanup signalling/join, and requires a broad subsecond bound against its 100 ms budget/two-second fail-safe. The direct-child timeout has a similarly broad bound. These are deadline assertions, not performance benchmarks. The scoped writer is joined before test assertions; no detached grandchild, numeric PID cleanup, PATH mutation or Python dependency is used.

**P2 progress issue in the first fix, closed:** nonblocking reads followed by a fixed 25 ms sleep still failed the backpressure test on both OSs. Tiny producer writes and socket accounting can require many drain cycles; readiness-based wakeup avoids imposing that delay on every burst. My first source review did not predict this remaining progress failure. The failed Rust run was preserved and the implementation corrected without weakening tests. An independent diagnostic supported the mechanism but is not treated as sole-cause proof or Rust qualification.

The final collector uses nonblocking reads, checks the deadline within draining, caps bytes before appending, and polls readiness while output remains open. EOF uses bounded status polling instead of polling a permanently ready EOF descriptor. Successful direct-child exit, complete EOF and valid UTF-8 are all required to report devices. A reaped leader with an incomplete stream reports unknown conservatively. Cleanup only kill/waits the directly owned unreaped child; observed `ECHILD` avoids signalling. Stored/output bounds, fixed production executable/arguments and five-second budget remain. No CPU/RAM API or placement policy is added.

No additional actionable P1/P2 issue remained after final source review. Millisecond conversion floors then clamps to 1–25 ms; it is not general rounding up. Spawn, scheduling and kill/wait cleanup do not become hard real-time operations.

## Actual run evidence

Verified the [qualification evidence](../evidence/gpu-probe-collection-2026-10-17/qualification.json) against actual job metadata, checked-out source, raw logs, named results and complete sample objects:

1. [RED 39089226768](https://github.com/nessalabs/workspace-sdk/actions/runs/39089226768), source `f75788410ccfef43bbfdbf4109b865f2277b059f`: both OS jobs failed held-writer deadline and backpressure cases; the other three cases passed. This stopped at the first all-feature unit stage and was intentionally not accepted as a shipment.
2. [First fix 39089468717](https://github.com/nessalabs/workspace-sdk/actions/runs/39089468717), source `84d2ae63e59cdefdb0f5412f4d8e2307fda15c78`: both OS jobs still failed backpressure with zero versus 4000 rows; the other four cases passed. This prompted the readiness correction.
3. [Readiness-fix run 39089774386](https://github.com/nessalabs/workspace-sdk/actions/runs/39089774386), final source: all five new tests passed under both feature selections on both OSs; macOS completed its full gate. Ubuntu failed its final loaded PTY performance resize at line 183 with `EAGAIN`, after normal checks passed. The failure remains preserved. Its exact native I/O cause is unproven; GPU collection is not on that resize call path. No test/source/dependency change preceded full revalidation.
4. [Full revalidation 39090003596](https://github.com/nessalabs/workspace-sdk/actions/runs/39090003596), the same final source: both Ubuntu 24.04 and macOS 15 successfully completed `scripts/quality-gate.sh`. Independently counted every new named case twice per OS: **20 new-test executions**. Formatting, warning-denying all-target/all-feature Clippy, both feature selections, examples, warning-denying docs and managed/PTY qualification passed.

The five-test file is byte-identical to actual RED source, SHA256 `c6ded4cf23cc570b4bfbfbcea099a4e1515bc4857f77fe72bbcdfff41631715b`. Additional oracles cover successful/nonzero exits, invalid UTF-8, exact 65,536-byte acceptance/65,537-byte refusal and direct-child timeout. The old-code run did not fail every case; no broader RED claim is made.

Verified raw-log hashes:

- RED: `c12b51b7d3f2ead2128f3e2c93ca92752c81223ad3fd8b352883d0a62938e716`.
- First fix failure: `bca1d0f5f4d42b1968aafd27e8c1af9ea9e7d9b5524d816d801e6c15703294ee`.
- PTY qualification failure: `1d5782608136189d1c2162af5fbe8b99ee562c0acce1c4c486c6e606be0a0c37`.
- Final success: `dec946210b0b8141ad0487a3adf43cce272fc3dac5e95812284ae4aa8b9ec20d`.

Each successful OS log contains full three managed and fifteen PTY samples matching the evidence objects exactly, on hosted debug-build Linux x86_64 and macOS aarch64. These are existing runtime scenarios, not a GPU collection throughput benchmark, statistical regression baseline or hardware qualification. No evidence discrepancy was found. This reviewer performed no builds/tests during acceptance and edited only this review record.

## Limits

- Shell/socket fixtures qualify child/stream boundaries, not NVIDIA hardware, driver behavior, provider transport, deployment or actual descendant cleanup. An empty device list remains unknown/no evidence, not proof GPUs are absent.
- The collector owns one direct child. It neither discovers nor terminates a descendant process group. It does not signal after observed reap; external ownership changes, `ECHILD`, fcntl and I/O error branches were source-reviewed rather than separately fault-injected.
- The deadline bounds collection logic and readiness waits; process spawning, OS scheduling and owned-child cleanup have no hard real-time guarantee.
- Successful revalidation does not prove the prior PTY `EAGAIN` harmless or root-caused. It is retained as an unexplained qualification failure, not hidden by combining partial jobs.
- Resource capacity semantics, reservations, idle-CPU/available-RAM policy, retry/handoff authority and sibling PTY implementation remain unchanged.
