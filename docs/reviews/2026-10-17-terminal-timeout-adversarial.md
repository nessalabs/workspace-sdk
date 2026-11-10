# Terminal control timeout classification: adversarial acceptance

Decision: **FINAL PASS** for source `85c2807c15e3c26f8732a6265f828194727f1151`, including production fix `28daf23061dcaac2111d2cacd952676a8f0dbfed`. No unresolved P1/P2 finding affects this increment. Reviewer: adversarial subagent, separate from implementation and code-quality review. This reviewer reviewed tests/source/evidence without editing production or test files or running builds.

## Finding and source review

**P2, closed:** blocking terminal-control socket I/O can expire through `WouldBlock` or `TimedOut`, after the code's pre-syscall deadline check. Previously those syscall results propagated as generic I/O errors, losing the existing stage-specific timeout explanation. The actual corrected RED exercises this behavior on both platforms.

The fix maps only read/write `WouldBlock` and `TimedOut` to the existing fixed timeout diagnostics. Read `Interrupted` now retries within the same original absolute deadline and retained partial frame. Other errors still propagate. There is no deadline extension, request retransmission, operation retry, new wire variant or frame-size change. Input exchange failures remain `Unknown`; preconnection refusal rules remain unchanged. A timeout does not establish zero delivery or permission to replay.

The tests use actual owned Unix socket pairs: silent held peer, partial frame without newline, confirmed send-buffer backpressure, complete frame, exact 128 KiB/+1 boundary and write-half EOF. The blocked-write fixture first delivers a prefix outside `write_frame`, then invokes the helper on the remainder; it does not prove the helper itself delivered that prefix during this invocation. Producers use bounded socket writes and scoped joins before assertions. No PID signalling, global signal handler, CWD change or detached process fixture is introduced.

## Fixture findings and disposition

The initial macOS EOF fixture fully closed its peer. Setting `SO_RCVTIMEO` could then fail with `EINVAL` before the read observed EOF. The corrected fixture retains the peer and shuts down only its write half, isolating the intended EOF contract. This does not fix or newly classify fully closed-peer timeout-setup errors.

First fixed-code validation found the same closure race in the size-boundary producer: finishing `write_all` dropped the writer before the reader's next timeout setup. The final helper returns the writer in its scoped join result, retaining ownership until framing completes. Exact size assertions and bounded producer cleanup remain. No further production change was required. Both fixture failures are retained, not attributed to the timeout fix.

No additional actionable P1/P2 issue remained after final source review. Only the three timeout regression bodies are identical to corrected RED; the whole final test file differs because of the boundary helper correction.

## Actual evidence

Independently verified the [qualification evidence](../evidence/terminal-control-timeout-2026-10-17/qualification.json), all four actual run metadata/raw logs and source identities:

- [Initial RED 39090735789](https://github.com/nessalabs/workspace-sdk/actions/runs/39090735789), source `5d2d2599432d97010fca1374ee5d7e3d05dfb8dd`: three expected `Io(WouldBlock)` timeout failures on both OSs; macOS additionally failed the full-close EOF fixture with `EINVAL`. Initial all-feature unit stage only.
- [Corrected RED 39090901531](https://github.com/nessalabs/workspace-sdk/actions/runs/39090901531), source `d5c5065b37e9fd933c616676160f3be27e9eecfb`: exactly three timeout failures and three framing passes on each OS, against old production logic. Initial all-feature unit stage only.
- [First fix 39091057439](https://github.com/nessalabs/workspace-sdk/actions/runs/39091057439): Ubuntu completed its full gate; macOS passed timeout cases but failed the over-limit boundary helper on `EINVAL`. This prompted the retained-writer correction described above.
- [Final full gate 39091296657](https://github.com/nessalabs/workspace-sdk/actions/runs/39091296657): both Ubuntu 24.04 and macOS 15 checked out the exact final source and successfully completed `scripts/quality-gate.sh`.

Counted all six new named tests passing twice per OS: **24 new executions**. Also independently counted five retained input-uncertainty/no-replay tests twice per OS: **20 retained executions**. The final gate passed formatting, warning-denying all-target/all-feature Clippy, both feature selections, examples, warning-denying docs and managed/PTY qualification. Complete three managed and fifteen PTY sample objects per OS match the raw logs exactly. These hosted debug-build Linux x86_64 and macOS aarch64 samples are existing runtime qualification, not a statistical baseline or control-latency improvement claim.

Verified raw-log SHA256 values:

- Initial RED: `40344fb7a488092889142a33946c052cfc749f523c830bf5d98e50f66aa14199`.
- Corrected RED: `a2b40dbdea4240361d5c3a1def9f8da404ea2d8c961f91239e6138d523bfac08`.
- First fix validation: `ac38ad8b270f4cd2956219e598f61499fad88a7cd625dd0f54b5fa0aac458f80`.
- Final success: `1ee05ff19376dcd20c3aaa0af96392dcced70ff937bd6e79c4c74ad5097c6ea0`.

No evidence discrepancy was found. This reviewer changed only this review record during acceptance.

## Limits

- The historical loaded PTY resize `EAGAIN` origin remains unproven. This fixes socket-error classification; it does not establish a repair for native control starvation, scheduling delays or transport service latency.
- Private framing helpers return `Error::Timeout`; existing supervisor fault conversion still turns that into `State` across the wire. No new public typed timeout guarantee is claimed.
- EOF coverage uses write-half shutdown with a retained peer. Fully closed macOS peer timeout-setup `EINVAL` remains an I/O error. The boundary helper's final choreography was not rerun against old production logic; the three actual timeout regression bodies remain unchanged from corrected RED.
- Interrupted-read retry, repeated-trickle deadline behavior and other I/O branches were source-reviewed, not newly fault-injected. Actual RED observed `WouldBlock`; the alternate `TimedOut` branch is portability logic rather than separately observed evidence.
- Timeout remains ambiguous about dispatch and partial delivery. No operation retry/replay permission, input uncertainty, cancellation, handoff or native PTY dependency behavior was relaxed.
- No live provider, cross-host transport, hardware persistence or deployment qualification is claimed.
