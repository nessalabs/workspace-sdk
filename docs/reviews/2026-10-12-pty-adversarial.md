# PTY integration: independent adversarial review

Reviewer: `retained_eof_adversarial`, 2026-10-12. Scope: the whole workspace SDK
PTY integration diff against `94d4e20a94e606a3b86576f07f5b6727989288bc`, including
terminal domain types, backend/runtime adaptation, supervisor ownership and
control, durable launch/output records, Boat private transport, real-process
acceptance tests, adversarial fixtures, and performance qualification. Read
`coding_standards.md`. The separately owned PTY dependency was not edited.
The root agent ran builds; this reviewer inspected actual source and logs and
authored adversarial tests. Performance harness review was independent of its
author.

## Findings, fixes, and re-review

1. **P1, closed:** terminal launch secrets and input initially entered Boat
   command text. Exhaustive protocol classification now sends both operations
   through private file transport. Exact command-body and typed-payload checks
   cover secrets, non-ASCII input bytes, cleanup, and no replay. The two initial
   regressions failed in `private-transport-red.log`; all seven transport tests
   passed in `private-transport-green.log`.
2. **P2, closed:** durable incomplete-drain evidence was ignored by public output
   reads. Output now reports the persisted gap, including at settled EOF after
   reconnect, without inventing bytes or changing cursors. The red adversarial
   run reproduced the missing gap; subsequent runs passed. The marker-injection
   test checks the durable boundary; the separate real held-descendant case
   exercises native bounded drain and cleanup.
3. **P2, closed:** a wrong success variant after delivered input returned an
   ordinary error. It now returns `Unknown`. Real input delivery followed by
   injected timeout, wrong variant, and impossible accepted count establishes
   no automatic replay and unchanged one-time child acknowledgement.
4. **P2, closed:** a non-executable PATH shadow hid a later valid program.
   Effective-ID `faccessat(..., X_OK, AT_EACCESS)` now determines accessibility.
   Regression includes both mode 0600 and owner-denied/group-executable mode
   0010 before the valid executable; the latter rejects the insufficient
   any-execute-mode-bit fix.
5. **P2, closed:** the performance cancellation oracle accepted any exit code.
   It now requires Running immediately before cancellation and exact canceled
   exit code 130 afterwards. Source re-review and `performance03.log` passed.

The initial adversarial run also exposed two test assumptions, not product
failures: local lifecycle refusal returns a Blocked deletion receipt, and
macOS socket tests must explicitly restore blocking mode and consume the error
reply before observing EOF. Corrected without weakening the 1200 ms trickle
deadline or three-second cancellation timeout. Owner-death checks establish
Lost, released active lock, dead leader and descendant, unchanged launch count,
stable same-key identity, rejected mode changes, and retained durable claim.

## Final source review

Request frames retain an absolute 250 ms budget and 128 KiB size bound; output
pumping is finite before control service and cancellation checks. The reply
budget is separately bounded at two seconds, allowing the native 500 ms operation
and queue service. Socket connection backlog waits use bounded socket2 connect.
Ambiguous delivery maps to Unknown; no automatic input retry was introduced.
Private working-directory changes occur only in dedicated CLI processes.
Cleanup finishes before terminal state publication. Absent owner evidence remains
Lost and cannot authorize relaunch or workspace deletion.

Performance test review verifies numbered ASCII bytes at absolute offsets,
exact 128 KiB retained tail, initial retention gap followed by contiguous reads,
stable empty completion, continued output during controls, observed child input
and dimensions, and exact cancellation outcome. `performance03.log` passed all
15 sessions. Resource observations are single supervisor-only snapshots, not
peak-memory, leak, or statistical regression proofs.

## Mechanical evidence and decision

`acceptance-green04.log` passed four native and eight adversarial cases.
The final gate subsequently passed those suites in its all-feature phase but
`work/pty-integration/final-gate.log` then failed with E0277 converting socket2
Socket into UnixStream in another feature configuration. **Whole-scope
acceptance is PENDING**, despite closed behavioral findings, until the complete
gate passes after that build fix and the final source is re-reviewed.

The root fixed the feature-dependent conversion by transferring the socket into
`OwnedFd`, then into `UnixStream`. Re-review confirms single RAII ownership with
no leaked or duplicated descriptor and unchanged bounded-connect behavior.
`final-gate02.log` is the complete affected rerun, still pending at this entry.
The rewritten integration guide accurately describes ownership, gap semantics,
control budgets, private transport, and local-only qualification limits.

## Final whole-scope acceptance

**PASS.** Re-read the final source, including bounded socket connection via
socket2/OwnedFd, effective-ID PATH lookup, separate response budget, private
transport classification, output-gap propagation, and corrected test oracles.
No unresolved P1/P2 adversarial finding remains for the integration scope.

`work/pty-integration/final-gate02.log` reaches the script's successful mechanical
completion marker. It includes formatting, warning-denying all-target Clippy,
all-feature and no-default-feature tests, warning-denying rustdoc, supervisor
build, existing managed-run performance, and new PTY performance. Both feature
configurations pass all four native acceptance and eight adversarial cases.
The final PTY performance run passes in 15.96 seconds with 15 sessions, exact
131072-byte retained tails, child-observed input and resize, and the strengthened
cancellation oracle. Its report timestamp is 1791853103 on macOS/aarch64 with
debug assertions enabled. These results close the earlier feature-dependent
socket build failure and qualify the actual final control implementation.
Independent SHA-256 verification of all 236 entries in
`work/pty-integration/final-source.json` found no changed source/configuration
file after the completed gate. The final gate log SHA-256 is
`143e0cba2b1f9ec31d63f40130a3e11c1ed0e2f5d1e01d4875d8752437a4bc4a`.

This is explicit independent adversarial acceptance of the whole current PTY
integration, including its performance harness. The separate code-quality
review decision remains required alongside this decision and the completed
mechanical gate. Historical evidence-label publication has its own review.

Remaining limits: local macOS real-process evidence does not establish live
remote-provider PTY behavior, real account login, rendering, Linux runtime
behavior, peak saturation, or long-running resource growth. Public output is
lossy UTF-8 text with raw-byte cursors; it does not establish binary replay
fidelity. Input may be echoed by workloads. No build or dependency edit was
performed by this reviewer.
