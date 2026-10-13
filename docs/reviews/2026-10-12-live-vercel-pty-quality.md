# Live Vercel Linux PTY qualification — quality review

Final independent quality decision: **PASS** for the explicitly scoped
unprivileged Linux cloud qualification below. No unresolved P1/P2. The final
evidence supersedes earlier pending checkpoints; Boat compute remains untested.

Reviewer: independent `performance_gate` agent. The root owns cloud mutations,
builds, and execution. This reviewer makes no production edits and runs no builds.
The separate adversarial reviewer must provide an independent final decision.

## Scope and acceptance conditions

The requested Boat compute test cannot run while that compute is unavailable.
The user authorized Vercel Sandbox as an alternative. This qualification can
establish behavior of exact Workspace SDK commit
`e3465f9afa71cac37dd23326651c8c6ddf939f45` and its pinned public PTY dependency
inside real Linux cloud compute. It does **not** certify the Boat provider,
a Vercel provider adapter, remote provider upload/command transport, or real
account sign-in. Existing public `LocalProvider`/`BackendRuntimeHost` operations
inside the VM are the implementation under test.

Required evidence:

1. Exact source archive/manifest and Cargo.lock identity, pinned PTY revision,
   VM identity/environment, compiler, and built supervisor identity. Build with
   the lock file rather than silently replacing dependency versions.
2. Complete unchanged `scripts/quality-gate.sh` inside the VM, with exit status,
   full logs and both performance JSON reports. Do not infer a passing gate
   from a partial suite or an empty test filter.
3. A bounded temporary harness using public APIs, independently reviewed before
   execution. Separate completed cloud command requests must start, reconnect
   and interact with, then cancel/clean up the same retained process. Saving the
   descriptor/process reference/cursor must not save synthetic input bytes or
   credentials. Same-key retry must prove one launch, and child observations
   must establish actual input and resize behavior.
4. Retained-output completion, exact cancellation outcome, owned workspace and
   process cleanup, evidence retrieval, and explicit VM stop confirmation.
   Failure paths also require cleanup. The VM's existence alone is not evidence
   that a detached SDK supervisor survives between cloud command requests.

The cross-request harness complements rather than replaces the unchanged real
PTY acceptance/adversarial tests and performance qualification. A roughly
five-minute workload bound is appropriate for several tool calls; a short test
fixture deadline could otherwise be mistaken for cloud ownership failure.

Status: **PENDING**. This initial plan review does not claim any live execution,
mechanical success, or remote-provider certification. Final findings, actual
evidence, residual limitations, and re-review decision will be recorded below.

## Public probe source review

Read `examples/pty_live_probe.rs` independently of its author. It uses public
provider/runtime-host APIs and persists qualified descriptor/process/cursor state.
Its exclusive owned 0700 state root, 0600 state files, controller lease, and
atomic fsynced saves make control ownership explicit. The workload is its own
bounded example mode, validates all three terminal descriptors and foreground
group, records one launch, and acknowledges input and dimensions independently.
Boot identity and process start ticks avoid treating PID reuse as the same owned
workload. Exact transcript prefixes bound collection and detect echoed input.
The successful cleanup path checks exit 130, stable completed output, dead/zombie
original identity, and completed backend deletion. No provider implementation
or domain layering is bypassed.

**P2, open:** partial-phase state currently prevents cleanup. Start saves
`phase = created` with a process reference before readiness checks, but cleanup
accepts only started/reconnected phases. A reconnect can also deliver input
before saving its new transcript; cleanup then rejects the additional output
before reaching workspace destruction. Cleanup must attempt bounded cancellation
and destruction for known-owned partial work while retaining the failed oracle
as a failure. A stale pending save must not prevent attempting cleanup either.
Sent the finding to the root and independent example author; source acceptance
is pending that fix/re-review. Explicit VM termination remains the final cloud
resource fallback, not a replacement for claiming successful workspace cleanup.

Source re-review closes the P2 implementation issue. Cleanup now attempts
15-second bounded cancellation, runs the strict phase/transcript/identity oracle
in a separately awaited task with a 30-second bound and captured panics, then
attempts 30-second bounded destruction regardless of oracle failure. A partial
created/no-reference state also reaches destruction. Failed verification remains
`cleanup_failed_verification` and returns an error; it cannot be published as a
successful phase. Unique 0600 temporary state files replace the stale fixed
pending-name hazard. Supplemental supervisor identity continuity, active-owner
lock release, and independently absent project/control paths strengthen the
positive oracle. Source review: **PASS**, no remaining P1/P2 found. Requested
one deliberate lagging-state live recovery case to verify this repaired path:
nonzero oracle result together with confirmed owned-root removal. Live execution
and overall qualification remain pending.

## Linux credential precondition correction

The initial unchanged gate in the VM reached 15/16 adversarial cases and failed
the effective-owner permission search fixture. The root's direct OS probe
reported UID/GID 1000 with inherited/permitted/effective/bounding/ambient
capabilities `000001ffffffffff`; `faccessat` allowed an owner-owned mode-0010 file.
`CAP_DAC_OVERRIDE` legitimately bypasses the fixture's ordinary owner permission
expectation. Production lookup intentionally uses actual effective credentials,
so this evidence does not justify changing that lookup or weakening the test.

The approved setup correction is to retain the failing evidence and direct OS
probe, then rerun the unchanged gate under `setpriv` with bounding, inherited,
and ambient capabilities dropped and no-new-privileges enabled. Verify actual
capabilities are zero and the direct owner-mode probe is denied before accepting
the corrected run. The same credential envelope must wrap each separate probe
RPC and recovery/cleanup command. Final qualification is of unprivileged SDK
execution inside this cloud VM; it must not imply that the initial default
capability environment passed. Actual corrected-run evidence remains pending.

## Live receipts independently inspected

The saved capability-free OS probe exits zero: euid 1000, mode 0010,
`faccessat = -1` with errno 13, and all five capability sets zero. The initial
unchanged gate receipt retains exit 101. The corrected unchanged SDK gate
(`sdk-capfree-gate-receipt.json`) exits zero, as does the example build/check
(`probe-build-receipt.json`). Full downloaded logs and artifact identities
still require final inspection.

Three distinct live command receipts all exit zero under the same zero-capability,
no-new-privileges envelope:

- Start (`phase-start.json`), controller PID 23881, stdout cursor 40.
- Reconnect (`phase-reconnect.json`), controller PID 23906, stdout cursor 67.
- Cleanup (`phase-cleanup.json`), controller PID 23926, stable cursor 67.

The latter two requests independently check that the previous controller stopped.
The retained supervisor identity stays PID 23887/start tick 117595; workload stays
PID 23894/start tick 117596, with unchanged VM boot identity. Separate request
timestamps span approximately 53 seconds from start to cleanup. The exact
transcript establishes all three terminal descriptors, foreground ownership,
initial 80x24 dimensions, consumed input and observed 120x40 resize. Cleanup ends
with `cleaned` after the source's exact cancellation/completion/deletion checks.
This is evidence of persisted ownership across command requests, not merely
three helper calls inside one running controller.

The recovery receipts behaviorally close the cleanup P2. A second real successful
start is deliberately saved as `created`, cursor zero and empty transcript.
In a separate capability-free request, cleanup returns probe exit 1 and saves
`cleanup_failed_verification`. The outer assertion wrapper independently confirms
both owned project and control paths are absent, then exits zero. This expected
negative verification is not labeled a positive phase. It proves cleanup remains
reachable while the saved state lags. Source quality and these live behavioral
checks pass; final artifact review and explicit VM termination remain pending.

## Final artifact, mechanical and cleanup acceptance

Independently opened the downloaded evidence and compared its complete 281-file
tracked source manifest against `git archive` of
`e3465f9afa71cac37dd23326651c8c6ddf939f45`: every hash matched, with no missing or
additional committed files. The separately added probe source matches the local
reviewed example exactly, SHA-256
`1b83890199cdfa5e75f1473bc000f4ffedb04adcea5bafed41c248ea5129d734`.
No SDK production source or separately owned PTY dependency implementation was
edited. The SDK supervisor binary hash is
`126469b6dae408cfff098f52ed305021c11af5a1a11511999b93668b7dad9f28`;
the probe binary hash is
`1e8f55fbe21fd997f40b5746ac42ad0048b1bf090f0db612214e97b9a4f70152`.

The recorded ephemeral Vercel session ran in region iad1,
2 vCPUs, 4096 MiB, nonpersistent, 30-minute maximum lifetime. Image:
`vercel/sandbox/universal@sha256:c77f7436b9bc0a8b01ef7b09dbcea2de6a18a9fe2aafa89996d2da193da72cf0`.
Recorded runtime environment: Linux 6.18.49 x86_64, rustc 1.98.1
(48a229cea 2026-09-01), euid 1000, all capability sets zero and NoNewPrivs 1.

Both downloaded Linux gates are complete: `sdk-capfree-gate.log` qualifies the
unchanged SDK; `sdk-probe-gate.log` qualifies the same source plus the reviewed
example. Each completes all steps of `scripts/quality-gate.sh`, including strict
Clippy/rustdoc, both feature configurations, supervisor build and both explicit
performance targets. Each contains 266 successful phase executions, no failures,
and five ignored entries in ordinary phases; both performance targets then run
explicitly. These totals include repeated tests/doctests, not 266 distinct tests.
The independent local `local-full-gate.log` also ends in mechanical success.
The failed default-capability gate is retained as `sdk-full-gate.log`; it is not
reclassified as a success or omitted from the evidence.

Final cloud performance reports are Linux/x86_64 with debug assertions enabled,
timestamps 1791858884 (pipe) and 1791858889 (PTY). The 15-session PTY qualification
passes in 5.28 seconds; every session retains exactly 131072 independently
verified bytes, observes input and dimensions, confirms exact cancellation,
and verifies final stable output/cursors.

| Initial population | Samples | Observed input ms | Observed resize ms | Cancel ms | Supervisor RSS KiB | Numeric descriptors | Observed bytes/s |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | 3 | 1.860–11.170 | 27.971–28.344 | 51.892–52.823 | 7528–7620 | 17 | 880950–884535 |
| 4 | 12 | 5.246–17.656 | 24.778–37.296 | 52.853–60.154 | 7396–7688 | 17–18 | 859743–879039 |

The three pipe samples retain 16384 bytes per stream; final stream offsets are
196608/192512/196608. Launch is 3.020/3.077/2.066 ms, first output
19.072/17.525/18.010 ms, reconnect 0.049/0.037/0.039 ms, same-key retry
1.482/1.821/1.570 ms, loaded read 1.440/1.635/1.520 ms, and cancellation
52.124/51.934/51.866 ms. These workload-paced observations include polling and
probe overhead; they are not maximum throughput, peak memory, leak proof,
cross-host speed comparison, or a statistical regression baseline.

After evidence retrieval, the stop API receipt reports both sandbox and session
`stopped`; an independent subsequent session lookup confirms `stopped`, no
routes, stoppedAt 1791859246941. Positive workspace deletion and negative-case
resource removal were independently established before VM stop. The retained
records therefore demonstrate completed cloud cleanup rather than relying on
eventual VM timeout.

Evidence SHA-256 (raw artifacts are retained under `work/live-vercel`):

- Downloaded 31769-byte archive: `ec87e5a84e8825e73d0fb864df509c8bbcdae5d40bbf163a57462aaab11f1295`.
- Environment/281-file manifest: `72d96f05e9ca6690fc71d1913487861fb44c7d908969f26e86037a1c23fa76b0`.
- Original failed Linux gate: `0abee161ca35f2662bf6ff90826798bee4d29d4cb1e05c5c85acae66f1440ddc`.
- Unchanged capability-free gate: `f30b958bb522a8ca8b190b1d0afe2b9328063a3a708fc7d7fd054bb6a69b0c22`.
- Gate including probe: `7e897bd1d57254882ab43f00a743f242c2d5f9c64b0ee917623e94d52d2fdce1`.
- Final PTY report: `3253cc5381e8bed10b9d997324346446ac2ad47e00639de20b5d079dbe61964d`.
- Final pipe report: `1c6c5243aff517bb67705a05d91c85f2eab89d714ece7c85f2495c24b186a9a8`.
- Stop receipt: `52f2731e5d2f138204556d3f8fa0a754eaee270d675c543beeb0cf989e368d23`.
- Independent stopped-state lookup: `67f7e4bf06923ca3e7a7dc4ecbc6994f88a14b7cde7fdc25fc829ef5ab4a4c82`.

Final code-quality and scoped qualification decision: **PASS**. The cleanup P2
is fixed, independently re-reviewed, and behaviorally verified; no unresolved
P1/P2 remains. The separate adversarial decision is recorded in
`2026-10-12-vercel-live-adversarial.md`. This proves the public SDK's local-provider
PTY implementation on unprivileged real Linux cloud compute, including ownership
across separate cloud commands. It does not certify Boat compute, a Vercel SDK
provider adapter, default-capability Vercel execution, real sign-in, remote
credential transport, renderer behavior, or extended resource/performance limits.
The reviewer performed no builds, cloud mutation, or production edits.

## Publication evidence and prose re-review

Publication scope/provenance decision: **PASS**, no material P1/P2 or credential
disclosure found. Reviewed the new evidence README section, integration overview,
`vercel-linux-pty-live.json`, and `vercel-linux-pty-performance.json` against the
retained source/receipts. Public phase fields exactly match the downloaded raw
phase records. The published numeric performance file is byte-identical to its
downloaded original. The standalone manifest digest correctly hashes the
281-entry map serialized with sorted keys, compact separators and no trailing
newline; it is distinct from the complete environment-record digest.

The prose now accurately says capability-free SDK execution **inside** the VM,
without implying that the VM's default credentials were globally changed. It
describes retrieval of full gate logs and successful-run artifacts rather than
claiming every original diagnostic was re-fetched. The original capability and
permission observations are separately labeled operator-observed transcriptions
in `work/live-vercel/diagnostic-observations.json`; the later API refused their
re-fetch after VM shutdown. This limitation does not turn the original failed
gate into a pass or invalidate the retained capability-free probes and runs.
The public summary omits cloud resource IDs, boot/workspace identifiers, input
challenge text, and credentials. Unnecessary ephemeral cloud command/session IDs
were also removed from this public review record; private receipt names and
hashes retain the audit trail. No source or test oracle changed during this
publication review, so the completed mechanical evidence remains applicable.
