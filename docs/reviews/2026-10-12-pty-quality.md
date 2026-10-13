# PTY adapter code-quality and performance review

Final decision: **PASS**. The final evidence entry below supersedes the earlier
pending checkpoints and targeted-run measurements for acceptance.

Reviewer: independent `performance_gate` agent. Scope: the workspace SDK PTY
integration against public `pty-runtime` revision
`c68837e2b7bf1c9009e1a4587a1a58107ec0425c`, with default features disabled. The
separately owned dependency implementation was not edited. This reviewer authored
the performance harness; independent adversarial review must also cover that
harness. Production review and performance authorship are stated separately.

## Production design review

Read the actual changed terminal domain values, backend port and implementation,
runtime host adapter, supervisor protocol/dispatch/run records/workload path, and
all three new terminal modules. Applied Clean Code, SOLID, dependency direction,
cohesion, lifecycle ownership, and appropriate pattern judgment.

The execution domain owns validated dimensions and input outcomes, independent
of native implementation types. `IRuntimeHost` delegates through the narrow
`IBackend` boundary and retains workspace-qualified process references. The
managed backend owns protocol adaptation and validates returned input counts.
The detached supervisor owns the runtime/session; ordinary request processes own
only bounded control exchanges. The existing durable launch claim incorporates
terminal mode and dimensions. Separate command adaptation and transient control
modules keep platform/transport details out of domain and runtime orchestration.
The concrete owner and delegation design does not need another factory or
strategy abstraction for this scope.

Input acknowledgement uncertainty becomes `Unknown` without replay. The private
transport classification is exhaustive and includes both terminal launches and
input. Reader pumping is finite between control opportunities. Retention uses
absolute native offsets, and incomplete draining is represented as a gap.
Resource ownership and local helper boundaries are coherent. No actionable
P1/P2 design finding in the production code read so far.

## Evidence and decision

Status: **PENDING**, not an accepted quality gate. Available evidence as of this
entry includes `work/pty-integration/acceptance-green01.log` (3 passed, 1 failed:
descendant drain gap assertion), `adversarial-green02.log` (7 passed, 1 failed:
trickled-frame fixture hit `WouldBlock`), and `private-transport-green.log` (the
targeted private transport tests passed). These failures require disposition and
affected re-runs; log names do not establish success.

The reviewer authored `tests/runtime_pty_performance.rs`, its standalone workload,
the explicit quality-gate invocation, and the PTY section of
`docs/performance.md`. The root agent owns compilation and test execution to
avoid concurrent build interference. No mechanical or performance pass is
claimed at this checkpoint. Final samples, environment, review results, source
identity, and remaining limitations will be recorded after actual execution.

The performance harness covers three populations each at one and four initial
simultaneous sessions, input observed by the workload, resize observed by a
child terminal query, cancellation under output, exact retained bytes by absolute
offset, and live per-supervisor RSS/open-descriptor snapshots. It does not
establish remote provider behavior, peak memory, long-running leakage,
rendering, maximum throughput, or a statistical speed-regression baseline.

## Performance harness adversarial finding

The independent `retained_eof_adversarial` reviewer found P2: the initial
cancellation oracle accepted any `Exited` value, so workload failure or timeout
could masquerade as successful cancellation. Fixed by requiring `Running`
immediately before cancellation and exact `Exited { code: 130 }` afterwards,
matching the runtime-host cancellation mapping. Re-review requested; affected
compilation and performance execution remain in the root agent's build lane.
The independent reviewer re-read the fix and explicitly closed the P2, passing
the performance harness source review. No performance execution pass is claimed
before actual run evidence is available.

## Loaded control deadline finding

`performance02.log` compiled the harness but failed when a loaded input returned
`Unknown` instead of the required acknowledged one-byte acceptance. Acceptance
and adversarial suites meanwhile passed all 4 and 8 cases respectively in
`acceptance-green04.log`; this is additional performance-boundary evidence.

Review identified an inconsistent deadline: native control acknowledgement may
take 500 ms, while the requesting CLI allowed only 250 ms for its response,
including time queued behind bounded output pumping. The root fix separates
the two-second response budget from the unchanged 250 ms incoming frame budget.
Source re-review confirms ingress anti-trickle protection remains intact, input
uncertainty still maps to `Unknown` with no replay, and the longer reply wait is
finite. The performance oracle is unchanged. `performance03.log` is the affected
rerun; its outcome must be assessed before accepting this fix.

The affected rerun completed successfully: `performance03.log` reports 1 passed,
0 failed, covering 15 sessions, in 15.65 seconds. Its JSON timestamp is
1791852530, macOS/aarch64, debug assertions enabled. Every session retained exactly
131072 bytes; input was acknowledged and independently observed, resize was
independently observed, cancellation returned the exact intended state, and the
settled replay oracle passed. Single-session observed input was 79–107 ms,
resize 285–306 ms, cancellation 126–176 ms. Four-session observations were
170–458 ms, 937–992 ms, and 349–543 ms respectively. Supervisor snapshots across
the run were 5824–6272 KiB RSS and 17–18 numeric descriptors; observed average
output rates were 0.61–0.75 MB/s per session, including startup and probe overhead.
These are raw smoke-qualification observations, not a regression baseline.
The separate response deadline fix is behaviorally verified by this run. Final
acceptance still requires the complete mechanical gate on the final source and
the separate adversarial review decision.

## Production source re-review decision

Production code-quality review: **PASS** for the current integration source,
including effective-ID executable lookup via `faccessat(AT_EACCESS)`, the separate
response deadline, and `socket2::connect_timeout` bounding connection backlog
waits before any input is sent. The socket abstraction preserves RAII ownership
and existing no-replay semantics without introducing custom raw-descriptor
polling. `check-bounded-connect.log` reports successful compilation. No unresolved
P1/P2 design finding remains. This source-review decision does not replace the
final mechanical gate or independent adversarial decision. The final mechanical
run must include the latest bounded-connect source and PTY performance test.

## Final gate and current-source acceptance

`work/pty-integration/final-gate02.log` completed the full
`scripts/quality-gate.sh` with exit zero: formatting, all-feature/all-target
Clippy with warnings denied, all-feature tests, no-default-feature tests,
warnings-denied all-feature rustdoc, supervisor build, and both explicitly
invoked performance qualifications. The log has 250 passed phase executions,
zero failures, and five ignored entries across ordinary test phases; these are
not 250 distinct tests. Both ignored performance targets subsequently ran and
passed. Real PTY acceptance (4 tests) and adversarial behavior (8 tests) passed
in both feature configurations. The no-default phase emitted an unused private
transport classifier warning; this does not affect behavior or the completed
required Clippy configuration.

The first final gate exposed a feature-dependent `Socket` to `UnixStream`
conversion. The fix transfers through `OwnedFd` and then
`UnixStream::from`, preserving single ownership and automatic close without
adding unsafe raw-fd handling. This reviewer re-read that actual fix and the
published `docs/pty-integration.md` contracts. The full rerun above verifies
both feature configurations. All 236 entries in
`work/pty-integration/final-source.json` independently matched current files
after the gate. No implementation file changed during this final review.

Final PTY report: `target/runtime-pty-performance.json`, timestamp 1791853103,
macOS/aarch64 with debug assertions. The host is Apple M5, 24 GiB memory,
macOS 26.6 (25G72), rustc 1.98.1 (48a229cea 2026-09-01). Competing host workload
was not controlled, so the report remains a local smoke observation. The test
passed in 15.96 seconds and includes three single-session samples plus twelve
session observations from three four-session populations. All retained tails
were exactly 131072 bytes with validated sequence/offset content, explicit
initial gaps, contiguous subsequent pages, stable completed rereads, observed
input and size changes, and exact cancellation outcomes.

| Initial population | Launch ms | Ready ms | Observed input ms | Observed resize ms | Cancel ms | Observed bytes/s | Supervisor RSS KiB | Descriptors |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | 26.777–28.244 | 183.139–351.581 | 90.661–115.008 | 275.113–327.349 | 124.456–176.685 | 578782–670598 | 5824–6128 | 18 |
| 4 | 27.429–97.510 | 188.709–280.286 | 163.810–498.785 | 930.469–1060.904 | 340.828–551.294 | 708443–751399 | 5952–6304 | 17–18 |

Final pipe report: `target/runtime-performance.json`, timestamp 1791853087,
same platform/build mode. Three samples passed, retaining 16384 bytes on each
stream; absolute stream offsets were 331776, 319488, and 278528. Launch was
44.087/27.185/27.003 ms; first output 270.461/65.981/64.843 ms;
reconnect 0.057/0.048/0.036 ms; idempotent retry 2.771/2.637/2.181 ms;
loaded read 2.796/2.082/2.115 ms; cancellation 71.917/66.502/73.120 ms.

Evidence SHA-256:

- Final gate log: `143e0cba2b1f9ec31d63f40130a3e11c1ed0e2f5d1e01d4875d8752437a4bc4a`.
- Source manifest: `81db50c6ef006a6016d98489334afec4522ecbc6002900fe377cd83ae4430167`.
- PTY report: `651e6d1e59209adfe6573015e80f60632a657ccf785c62fa2059af344a7877ae`.
- Pipe report: `de101eeb5c6a1f9599bc23de32c7210156cb166d0c74c7d29d1a9a9b3fc07ff2`.
- Terminal control source: `c61635119293167b435e8f732ce50c2480ab64ac2d7d3e3ae92ea836079126d9`.
- PTY performance test: `e2786cd02b44d2b09904a19074a0cb94d83bb2827fd5b9834adb4cca6fc561cf`.
- Integration contract documentation: `09e81edc67559705a31990cb3680be1b06f08e29debc57562f1640165613c10c`.

Independent production code-quality and performance qualification decision:
**PASS**, with no unresolved P1/P2 findings. Independent quality review of the
reviewer-authored performance test is separately recorded as PASS in
`2026-10-12-pty-performance-oracle-quality.md`; the separate adversarial reviewer
records whole-scope PASS in `2026-10-12-pty-adversarial.md`. Together with the
completed mechanical gate, these satisfy this change's required quality gate.
No reviewer build was run and no separately owned dependency implementation was
edited. Remaining limits are unchanged: real account sign-in, live remote and
Linux runtime execution, binary replay fidelity through lossy text, terminal
rendering, peak saturation/memory, and long-duration resource growth are not
established by this local qualification. Input can be echoed by the workload;
single supervisor resource snapshots and three iterations are not leak or
statistical regression proofs.
