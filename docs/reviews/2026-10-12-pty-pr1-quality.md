# PR #1 follow-up code-quality review

Final independent code-quality and local mechanical/performance decision:
**PASS**, no unresolved P1/P2. The final evidence below supersedes earlier
pending checkpoints. Hosted CI on the follow-up commit is a separate merge gate.

Reviewer: independent `performance_gate` agent. Scope: production changes in
response to PR #1 review comments on commit
`287d4b9c1411b240784a973154593daf9c408fc1`, plus the two minor validation/report
path comments. The reviewer reads actual source and test evidence, makes no
production changes, and runs no builds. The root owns fixes and mechanical
execution; the separate adversarial reviewer owns regression cases.

## Findings and required boundaries

- **P2, pending fix/re-review:** missing bare terminal executables fail before
  native child launch but become `Lost`, unnecessarily blocking reconciliation
  and lifecycle cleanup. A bounded, non-secret diagnostic and deterministic
  exit 127 are appropriate when executable resolution establishes this failure
  before native runtime/session creation. Existing durable claims must continue
  to return their saved state without re-resolving a removed executable.
- **P2, pending fix/re-review:** missing-run input errors become `Unknown` even
  though no control dispatch occurred. Preserve explicit refusal provenance.
  Error variants from `ITarget::invoke` alone are insufficient: provider or
  transport errors can use the same variants after delivery may have occurred.
  Only a typed supervisor refusal established before dispatch may become a
  definitive error. Missing/malformed/wrong-success acknowledgements, impossible
  accepted counts, and unclassified transport failures must remain `Unknown`
  without automatic replay. Adding a preflight inspect alone leaves a race and
  incurs another control operation; it does not solve error provenance.
- **P2, pending fix/re-review:** a socket unlink failure after shutdown can erase
  an established exit/cancel/timeout and cause the caller to persist `Lost`.
  Removing this stale socket is best-effort cleanup after owned processes settle.
  Merely writing the terminal state first and then returning the unlink error
  still allows the caller to overwrite it, so it does not fix the general case.

## Minor comments

Early launch validation parity is a valid, non-blocking improvement: the target
already checks secret environment validity and output limits, but malformed
terminal launches should fail before private transport. Any shared validation
must retain the distinct terminal maximum of 128 KiB and pipe maximum of 16 MiB.

The report-path comment correctly identifies direct test-binary invocation from
a different directory as a robustness issue. Its `CARGO_TARGET_DIR` explanation
is inaccurate: that variable changes Cargo build output, not the package-root
working directory of `cargo test`. The documented fixed report location can be
anchored to `CARGO_MANIFEST_DIR` without expanding the supported report schema or
adding a new configuration feature.

Status: **PENDING** until actual fixes, affected regressions, final mechanical
gate, and independent re-review complete. Prior green CI on the reviewed commit
does not establish acceptance of new fixes.

## First fix re-review

The actual first fixes preserve deterministic exit 127 before runtime creation,
retain bounded diagnostics, make socket unlink non-fatal after shutdown, and
introduce `InputRejected(SupervisorFault)` for dispatcher/pre-connect refusals.
Fault conversion is extracted from existing protocol code without changing
fault semantics. Managed input treats all unclassified target errors as
`Unknown`, including `Unsupported`; it propagates only typed refusal values.
`pr1-red.log` recorded 9 passes and 5 failures; `pr1-green.log` passed 14 cases.

**New P2, open:** the socket exchange forwards any successful `SupervisorValue`
unchanged, including `InputRejected`. A wrong-success acknowledgement received
after input dispatch can therefore forge the new pre-dispatch refusal provenance
and become a definitive error. Only `SupervisorValue::Input` is a valid success
value from an input socket exchange. Other success variants, including
`InputRejected`, must become `Unknown`; only the CLI's own pre-dispatch checks
may mint that envelope. Requested an independent delivered-input regression and
root fix. The initial green regression suite does not cover this boundary, so
acceptance remains blocked pending fix and re-review.

Root disposition of minor comments: deferred as non-blocking. Supervisor-side
validation already enforces request safety, and supported Cargo gate execution
has a stable package-root working directory/report location. The follow-up does
not add validation refactors or a new direct-binary invocation guarantee.

## Refusal-boundary fix re-review

The new P2 is **closed**. The socket exchange now permits only
`SupervisorValue::Input` as a valid successful input acknowledgement. A socket
`InputRejected` value or any other wrong-success variant becomes a local
exchange error and then `Unknown`. Only local dispatcher/pre-connect checks can
produce the typed refusal that the managed backend propagates. This preserves
the information boundary without another preflight round trip, retry, public
domain variant, or broad transport-error classification.

`pr1-refusal-red02.log` ran the two forged-refusal cases and both failed by
returning definitive NotFound/Unsupported after the peer consumed the exact
submitted bytes. The earlier `pr1-refusal-red.log` filter ran no tests and is not
red evidence. `pr1-green02.log` passed all 16 adversarial cases in 3.25 seconds,
including both consumed-input/no-replay regressions. These tests exercise the
real CLI/socket boundary rather than a direct helper assertion.

Source re-review also confirms the deterministic missing-executable receipt is
written before any native session exists, existing claims still bypass launch
work, and best-effort unlink cannot erase completion. The small fault conversion
helper belongs to protocol adaptation; domain types and provider dependency
direction remain unchanged. No extra abstraction or strategy layer is needed.
The integration documentation accurately explains exit 127 and typed refusal
provenance without claiming transport error names establish nondelivery.

Independent code-quality source decision: **PASS**, no unresolved P1/P2.
Whole-gate acceptance remains pending the final root-owned mechanical run on
this exact source and the separate adversarial decision.

## Final mechanical evidence and acceptance

`work/pty-integration/pr1-full-gate02.log` completed `scripts/quality-gate.sh`
successfully after the refusal-boundary fix. Formatting, Clippy across all
features/targets with warnings denied, all-feature and no-default tests,
warnings-denied rustdoc, supervisor build, and both explicitly invoked
performance qualifications passed. The log contains 266 passed phase executions,
zero failures, and five ignored entries in ordinary test phases; both performance
targets subsequently ran explicitly and passed. The 16-case adversarial suite
passed in both feature configurations. These phase totals include repeated tests
and doctests, not 266 distinct tests.

Independently compared all 236 entries of `work/pty-integration/pr1-source.json`
against current files after completion: every hash matched. No source edit was
made during this review. The prior complete gate before the additional refusal
regression is historical evidence only; `pr1-full-gate02.log` is the accepting
run.

Final PTY JSON timestamp 1791855021, macOS/aarch64, debug assertions enabled.
The 15-session test passed in 16.33 seconds. Three single-session samples observed
input in 104.243–136.923 ms, resize in 282.333–321.680 ms, and cancellation in
187.066–197.023 ms. Twelve observations across three four-session populations
measured 47.421–464.926 ms, 957.460–1148.264 ms, and 362.737–572.260 ms respectively.
Every session retained exactly 131072 validated bytes, with correct gaps,
contiguous offsets, stable completion, independently observed input/resize, and
the exact cancellation outcome. Supervisor RSS snapshots ranged 6064–6320 KiB
and numeric descriptors 17–18. Observed average output rates were
513177–639631 bytes/s, including startup, controls, and resource probes.

The pipe report timestamp is 1791855005. Three samples passed and retained
16384 bytes per stream, with final offsets 278528/319488/229376 on each stream.
Launch was 32.654/28.389/26.999 ms, first output 261.700/75.646/64.918 ms,
reconnect 0.040/0.057/0.037 ms, same-key retry 2.426/5.171/2.218 ms,
loaded reads 2.762/2.886/2.213 ms, and cancellation 78.365/78.139/74.946 ms.
These are current raw smoke observations, not a speed-regression baseline.

Evidence SHA-256:

- Accepting gate: `c1033fcffdcb93d71608bc5a7f2da22fbc926a3e5ed787c5f331b931e7e84bf9`.
- Source manifest: `635bbd39af9e6991dd0990cdaf11122f3adcfeedea3523b2e393d71c5b523ed8`.
- Forged-refusal red evidence: `cd4d7a0c647bf60b8cf6c31ac8d049092289c59ecb4fe675b2d5018670234ba4`.
- Sixteen-case green evidence: `c4337098d084c9e98bad5286af5472f33bbe76035643094d2cb7c644ca44c6ff`.
- PTY JSON: `2393544c563e3795658b5bec5579d733ed0d3ea3202288220ec1fa998fab7ce9`.
- Pipe JSON: `b9a7107d874fa4c85b78c6461dc0496afffc9c5997cf17e4cae4a5126b060b5f`.

Final code-quality acceptance: **PASS**. The three original behavioral findings
and the additional refusal-provenance P2 are resolved and re-reviewed with
meaningful red/green evidence. Remaining minor comments are explicitly deferred
above and do not block this change. The separate adversarial reviewer records
its decision in `2026-10-12-pty-pr1-adversarial.md`. No production edit or build was
performed by this reviewer, and the separately owned PTY implementation was not
changed. Existing limitations remain: real account login/live remote PTYs,
binary fidelity through lossy text, rendering, peak or long-term resource usage,
and statistically controlled performance are not proven. Hosted Linux/macOS CI
success on original commit `287d4b9` does not establish CI success for this
follow-up; the new commit's required checks must pass before merge.
