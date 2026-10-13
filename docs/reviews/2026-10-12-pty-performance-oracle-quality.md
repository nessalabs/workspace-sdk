# Independent PTY performance oracle review — 2026-10-12

Reviewer: `code_quality_gate`, separate from the performance-test author.
Reviewed `tests/runtime_pty_performance.rs`, its standalone
`tests/support/pty_performance_workload.rs`, shared fixture lifecycle, and the
qualification scope in `docs/performance.md`. No code edits or builds by reviewer.

The test verifies real public runtime-host operations rather than trusting a
successful API response alone: input acknowledgement comes from the workload;
resize acknowledgement comes from `stty`; ordered replay is independently decoded
from absolute byte positions in fixed-size records. It forces retention, requires
the initial retention gap, rejects later invented gaps, checks the exact 128 KiB
retained tail, confirms output advancement through controls, and checks stable
completion/cursors. Cancellation must report the expected terminal state.

Both one- and four-session populations retain active producers through concurrent
control work. Three iterations per population produce observations, not a
statistical baseline or peak-throughput claim. RSS and descriptor observations
are scoped to each live supervisor; docs do not claim system-wide memory or leak
proof from those samples. The outer fixture keeps workspace ownership through
panic/timeout and attempts bounded cancellation plus destroy. Fixed warmup is
workload establishment, followed by a bounded observable-load predicate.

Design/oracle decision: **PASS**, no actionable P1/P2 finding in this scope.
This source review does not establish a passing mechanical gate. The inspected
final gate log currently ends in a Socket-to-UnixStream compile failure. A
successful rerun and actual performance report remain necessary for acceptance.


Also reviewed the revised README link and `docs/pty-integration.md`. The contracts
now distinguish terminal ownership, replay retention versus incomplete draining,
uncertain input, local synthetic evidence, and unvalidated remote/native sign-in.
They do not infer authentication success from process exit or claim peak/leak
qualification from snapshots. No blocking documentation finding. Nit: “three
populations each” is clearer as “three iterations each” for populations one/four.
The follow-up `final-gate02.log` is underway after the initial compile fix.
