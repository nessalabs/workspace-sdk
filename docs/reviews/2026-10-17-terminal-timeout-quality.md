# Terminal control timeout classification: independent quality review

Reviewer: independent quality agent `/root/boat_cleanup_quality`, applying
`AGENTS.md` and `coding_standards.md`. Exact reviewed source:
`85c2807c15e3c26f8732a6265f828194727f1151`; production fix:
`28daf23061dcaac2111d2cacd952676a8f0dbfed`.

**FINAL PASS for this narrow classification change. No unresolved P1/P2 finding
affecting this scope remains.** Actual source and completed exact-source gate
evidence support this decision; the independent adversarial review remains
separately required.

## Findings, fixes, and re-review

| Severity | Finding | Disposition |
| --- | --- | --- |
| P2 | Socket read/write timeout can return WouldBlock/TimedOut from the OS, bypassing the existing absolute-deadline precheck and forwarding an unhelpful raw I/O error. | Closed. Match only these two error kinds to the existing stage-specific Error::Timeout messages. Interrupted reads continue under the same absolute deadline; other I/O errors retain their classification. |
| P2, fixture | Fully dropping the peer made macOS reject timeout setup with EINVAL before the intended EOF observation. | Closed before corrected red. Retain the peer and shut down only its write half. The legacy test name does not establish fully closed-peer behavior. |
| P2, fixture | The boundary producer could drop its peer before the reader's next timeout setup, producing the same macOS EINVAL instead of exercising the size limit. | Closed after first fix validation. Return the writer in the scoped producer's join result, retaining it until read_frame completes; then join and drop it. Boundary assertions and timeout regression bodies remain intact. |

The production diff is local to `terminal_control.rs`. It uses direct matches
at the existing I/O boundaries rather than introducing a runner, policy layer,
or transport abstraction. Frame limits, read/write budgets, framing behavior,
and operation dispatch remain unchanged. Retrying an Interrupted read is a
framing-loop continuation under the existing absolute deadline, not an
operation retry. Write interruption handling is unchanged apart from the
imported ErrorKind spelling.

Input exchange errors still become `InputOutcome::Unknown`; timeout is not
proof of no delivery and grants no retransmission permission. Private framing
helpers now return Error::Timeout, but existing `SupervisorFault::from_error`
still maps that variant to State across the wire. This change supplies the
existing stage-specific diagnostic through that path; it introduces no public
wire timeout variant. Non-timeout EINVAL remains an I/O error.

## Tests and actual evidence

The tests exercise real socket pairs: empty and partial reads, a confirmed full
write buffer, a normal newline frame, exact frame-size limits, and empty/partial
EOF via write-half shutdown. The write-pressure fixture verifies SO_SNDBUF,
pre-delivers a prefix, fills until actual WouldBlock, then calls write_frame on
the remainder. It checks typed timeout plus exact incomplete delivered bytes.
The prefix was fixture-written; it does not demonstrate that this helper
invocation itself delivered the prefix. Scoped boundary writers have a timeout,
are joined before assertions, and retain peer ownership through the read.

I independently checked final source and compared
`docs/evidence/terminal-control-timeout-2026-10-17/qualification.json` with all
four raw `../workspace-cloud-run-ID.{json,log}` records. Complete job metadata,
conclusions, checkout SHAs and raw-log SHA-256 values match. The final test-file
hash matches evidence. I compared all three timeout regression bodies directly
with corrected red source `d5c5065b37e9fd933c616676160f3be27e9eecfb`: they are
unchanged. The entire final test file is different because boundary peer
ownership was corrected afterward; no whole-file identity or final-helper
old-code red run is claimed.

- **39090735789, initial old-code red:** both OS showed the three expected
  Io(WouldBlock) timeout failures. macOS additionally failed the EOF fixture with
  timeout-setup EINVAL. The run stopped in the initial all-feature unit gate.
- **39090901531, corrected old-code red:** exactly three timeout failures and
  three framing passes occurred on each OS. The peer-half-shutdown fixture
  correction preceded the production fix. This run also stopped at the initial
  all-feature unit gate, rather than completing qualification.
- **39091057439, first fix validation:** Ubuntu completed the full gate. macOS
  passed the timeout regressions but failed the over-limit boundary helper with
  EINVAL, prompting the writer-retention correction. This is preserved failed
  qualification evidence, not a passing result.
- **39091296657, final exact-source full gate:** Ubuntu 24.04 and macOS 15 both
  completed successfully. Each of six new tests passed twice per OS under both
  feature selections: **24 new executions**. All five named retained input
  uncertainty/no-replay cases passed twice per OS: **20 retained executions**.
  Both actual logs reach the full script completion message, covering format,
  warning-denying Clippy, builds, both test selections, warning-denying docs,
  and managed/PTY performance qualifications.

No assertion or budget was weakened to obtain the final result. The fixture
changes isolate intended framing behavior from macOS full-peer-close timeout
setup, while production error handling remains the same fix.

## Performance and limits

I extracted and compared the complete performance reports with evidence:
**3 managed and 15 PTY samples per OS**, including environment and every sample
field. Managed cancellation observations were 53.02–53.33 ms on Ubuntu and
126.36–149.65 ms on macOS; PTY cancellation was 52.87–54.59 ms and
75.87–313.71 ms respectively. These hosted debug runtime observations are not a
statistical baseline or a control-channel latency guarantee.

The historical loaded PTY resize EAGAIN cause remains unproven. Error
classification does not repair or qualify control-service starvation. Actual
red tests observed WouldBlock on both OS; TimedOut portability handling,
Interrupted read behavior, repeated-trickle absolute deadlines, and other I/O
branches were source-reviewed rather than newly fault-injected. No operation
replay, budget increase, live provider, cross-host transport, deployment or
power-loss guarantee follows. This reviewer performed no local builds/tests,
source edits or commits; only this review record was written.
