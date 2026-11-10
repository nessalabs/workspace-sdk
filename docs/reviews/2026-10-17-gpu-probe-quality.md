# GPU probe collection: independent quality review

Reviewer: independent quality agent `/root/boat_cleanup_quality`, applying
`AGENTS.md` and `coding_standards.md`. Exact reviewed source:
`3224bb1d60f34041b571938dbfb6c66dbccb556f`.

**FINAL PASS for the native GPU probe collection change. No unresolved P1/P2
finding affecting this scope remains.** This decision includes source review
and actual full exact-source qualification. A separate adversarial review is
independently required. It does not declare the historical PTY resize failure
explained or repaired.

## Findings and dispositions

| Severity | Finding | Fix and re-review |
| --- | --- | --- |
| P2 | Waiting for leader exit before reading can block the writer on full stdout. A successful exited leader can also leave a writer open, making the subsequent blocking read exceed the collection deadline. | Closed. Drain nonblocking stdout concurrently with direct-child status checks; require successful exit, actual EOF, valid UTF-8 and bounded output. Post-exit WouldBlock yields an unknown observation. Actual old-code red and final unchanged-test green results support the fix. |
| P2, test oracle | Empty-result assertions alone could pass a collector that waits for the fixture failsafe before discarding output. | Closed before the actual red run. Collection elapsed time is measured before holder notification/join and must be below one second, versus the two-second holder failsafe and 100 ms collection budget. The 25 ms stalled-child case also has a broad one-second upper bound. |
| P2 | The first nonblocking implementation slept 25 ms after each producer burst, retaining a backpressure failure with the small-buffer fixture. | Closed. Poll actual stdout readiness for at most 25 ms or the remaining budget, then recheck deadline/status. After EOF use bounded sleep instead of spinning on persistent hangup readiness. Tests and budgets were unchanged. |

The source stays cohesive in `native_resources.rs`: a private concrete collector
accepts the spawned Child and budget, while the production `nvidia-smi` argv and
five-second budget remain unchanged. No generic runner, new public interface,
supervisor dependency, process-group abstraction, or sibling runtime change was
introduced. Nonblocking reads use an 8 KiB chunk, check the deadline during
draining, and reject bytes beyond 65,536 before appending. EOF before process
exit still requires a successful exit; nonzero status and malformed UTF-8
discard the observation. Existing row parsing remains unchanged.

Direct-child ownership is explicit. Only an unreaped directly owned child is
killed/waited during cleanup; observed reap and ECHILD avoid subsequent numeric
PID signaling. Descendants are not owned or cleaned by this probe. Setup,
fcntl, read and readiness errors discard the observation. ECHILD/fcntl/read
failure defenses were source-reviewed, not fault-injected. Spawn, scheduling,
and OS kill/wait cleanup are not hard real-time bounded operations. Poll timeout
milliseconds are floored then clamped to 1–25; the outer loop rechecks deadline.

## Actual evidence verified

I checked the source and test file against final HEAD and independently compared
`docs/evidence/gpu-probe-collection-2026-10-17/qualification.json` with all four
saved `../workspace-cloud-run-ID.{json,log}` files. Raw-log SHA-256 values,
complete job metadata/conclusions, and actual checkout SHAs match. The final
test-file hash matches both the evidence and the actual old-code red commit
`f75788410ccfef43bbfdbf4109b865f2277b059f` byte for byte.

- **39089226768, actual old-code red:** both OS jobs failed exactly the held-open
  writer deadline case and the backpressure case; the other three new tests
  passed, including the output boundary. Collection waited about two seconds
  despite a 100 ms budget, and backpressure returned zero devices versus 4,000.
  Both gates stopped at the initial all-feature unit failures; this was not a
  completed qualification.
- **39089468717, first nonblocking fix:** both OS jobs still failed backpressure
  zero versus 4,000; the other four new cases passed. This failure was retained
  and corrected with readiness waiting, not test relaxation. Fixed sleeps
  throttling short writes are source-supported diagnosis; the auxiliary Python
  diagnostic is not Rust/CI proof or an exclusive root-cause trace.
- **39089774386, readiness fix:** all five new tests passed twice per OS, and
  macOS completed the full gate. Ubuntu failed only the final ignored loaded
  PTY qualification at `runtime_pty_performance.rs:183`, resize returning EAGAIN.
  The collector is not on that resize path. Its cause remains unproven; this run
  remains a failed qualification, not a green result.
- **39090003596, unchanged full revalidation:** both Ubuntu 24.04 and macOS 15
  completed the full script, including formatting, warning-denying Clippy,
  builds, all-features and no-default-features tests, warning-denying docs, and
  managed/PTY performance qualification. Each of the five new names passed
  twice per OS: **20 new test executions**. No source, test, dependency or budget
  change separated the last two runs.

Fixtures use real directly owned shell children and real pipes/Unix streams.
The held writer is explicitly owned by a joined controller with a failsafe;
the backpressure fixture verifies small SO_SNDBUF relative to its 28,000-byte
valid CSV output. No PATH mutation, detached fixture descendants or raw PID
cleanup is involved. Cases also cover exact 65,536-byte acceptance, 65,537-byte
rejection, success/nonzero exit, malformed UTF-8 and a stalled direct child.

## Performance and remaining limits

I independently extracted and compared the complete performance report objects
against evidence: **3 managed and 15 PTY samples per OS**, with all sample and
environment fields. Managed cancellation was 51.76–617.27 ms on Ubuntu and
132.84–173.29 ms on macOS; PTY cancellation was 51.82–163.97 ms and
57.14–235.04 ms respectively. These hosted debug runtime samples are neither GPU
probe throughput measurements nor a statistical regression baseline.

The historical PTY EAGAIN observation remains unexplained; a successful unchanged
revalidation does not establish its cause. The accepted scope is the collector
change, with that residual explicitly retained. Empty GPU output remains an
unknown observation rather than proof of absent GPUs. Shell/socket fixtures do
not qualify GPU hardware, provider transport, deployment, process-group cleanup,
or physical persistence. No new resource-availability API or scheduling policy
is established. This reviewer performed no local builds/tests, source edits or
commits; only this review record was written.
