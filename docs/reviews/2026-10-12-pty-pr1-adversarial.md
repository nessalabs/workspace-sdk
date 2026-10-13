# PR 1 follow-up: independent adversarial review

Reviewer: `retained_eof_adversarial`, 2026-10-12. Baseline:
`287d4b9c1411b240784a973154593daf9c408fc1`. Read the actual PR comments through
`gh api repos/nessalabs/workspace-sdk/pulls/1/comments`, current production code,
and `coding_standards.md`. This reviewer owns adversarial test changes and this
record, not production changes or builds.

## Findings confirmed against source

- **P2, closed:** [missing executable](https://github.com/nessalabs/workspace-sdk/pull/1#discussion_r4253732575)
  returns before child admission, but the detached owner's error path publishes
  Lost. A definite launch rejection incorrectly prevents later lifecycle work.
- **P2, closed:** [missing run input](https://github.com/nessalabs/workspace-sdk/pull/1#discussion_r4253732579)
  loses definite pre-delivery rejection information. Conversely, an error's enum
  variant alone cannot establish provenance: transport NotFound or Unsupported
  after real delivery must remain Unknown. The old Unsupported exception has
  the latter weakness.
- **P2, closed:** [socket cleanup](https://github.com/nessalabs/workspace-sdk/pull/1#discussion_r4253730760)
  can replace a known exit with Lost. Merely publishing status before returning
  an unlink error would remain insufficient because the caller overwrites it
  on any returned error. Socket cleanup must not discard established completion.

## Independent regressions

Added six tests to `tests/runtime_pty_adversarial.rs`:

- Missing bare executable in an explicitly empty PATH: exact exit 127, bounded
  nonempty diagnostic, no admitted child identity, same-key reconnect preserving
  output and claim, and successful workspace stop.
- Correct host scope with nonexistent run ID: definite NotFound, no created run,
  and no secret delivered to a separate real terminal in that same workspace.
- Unlinked socket and directory replacing the socket pathname: exact observed
  exit 7 and final output, stable completion after reconnect, and successful stop.
- Actual child input followed by injected transport NotFound and Unsupported:
  Unknown, exactly one dispatch, one child acknowledgement, no replay or relaunch.

The prior pipe-control rejection and delivered-input timeout/wrong-variant/
impossible-count cases remain in the same suite. Test execution is root-owned.
Decision: **PENDING**; reproduce baseline failures, review production fixes, and
assess affected reruns plus the full mechanical/performance gate before acceptance.

Baseline run `work/pty-integration/pr1-red.log`: **9 passed, 5 failed** in
3.29 seconds. The missing executable and both socket cleanup cases observed
Lost; missing-run input returned Ok(Unknown); actual delivery followed by
Unsupported returned a definite error. Actual delivery followed by NotFound
already returned Unknown, so that case protects against an unsafe broad error
pass-through fix. This reviewer independently inspected the failure log.

The first fix rerun, `pr1-green.log`, passes all 14 cases in 3.15 seconds. Source
review confirms missing bare-program resolution writes a fixed, bounded
diagnostic and exit 127 before runtime creation; socket unlink is best-effort
after native shutdown; typed `InputRejected` carries pre-dispatch failures while
untyped transport errors remain Unknown. Those three original findings are
closed at the affected-test level.

## Additional provenance finding

The separate quality reviewer identified **P2, closed after re-review**: the initial typed-refusal
fix forwarded successful control-socket values unchanged. A peer that consumed
input could reply with `Success(InputRejected(...))`, minting the pre-dispatch
proof after dispatch. Only the request CLI's own pre-dispatch checks may create
that proof. Post-dispatch socket results must accept an input outcome or return
Unknown, including malformed or inappropriate success variants.

Added independent real Unix-socket peer regressions for forged NotFound and
Unsupported refusals after verifying the complete submitted input frame. Public
input must return Unknown, the peer must see no second connection, and the
original native owner must still cancel cleanly with exit 130. These tests do
not use internal hooks. Root owns baseline execution before the sanitizer fix.
The earlier full gate applies to the first fix and cannot accept this new
boundary until corrected, re-reviewed, and re-run.

`pr1-refusal-red02.log` runs the two added socket regressions and fails both:
the peer's forged NotFound and Unsupported responses became definite errors
after it consumed the input frame. An earlier mistaken filter ran zero cases
and is not evidence. The fix validates socket success values after dispatch:
only `Input` outcomes are accepted; any other variant becomes Unknown through
the existing exchange-error boundary. Source re-review confirms that only local
pre-dispatch checks can create a trusted refusal. `pr1-green02.log` then passes
all **16 tests** in 3.25 seconds. This reviewer inspected both logs and the exact
sanitizer code. All behavioral findings are now closed; final acceptance remains
pending the complete affected mechanical/performance rerun.

## Final acceptance

**PASS** for the complete PR 1 follow-up scope. All original and additional
P2 findings are closed, and no unresolved P1/P2 adversarial finding remains.
Final source re-review confirms refusal provenance is established only before
dispatch, missing executable lookup never claims a child was admitted, and
socket cleanup cannot replace established completion with Lost.

`work/pty-integration/pr1-full-gate02.log` reaches the successful mechanical
completion marker. Formatting, warning-denying Clippy, both feature test
configurations, warning-denying rustdoc, supervisor build, and both performance
qualifications pass. Both feature configurations include all four native
acceptance and sixteen adversarial cases. The managed-run performance test
passes in 1.38 seconds; PTY performance passes in 16.33 seconds with fifteen
sessions and exact 131072-byte tails. The final PTY report records timestamp
1791855021, macOS/aarch64, debug assertions enabled. This reviewer inspected the
completed log and report and independently verified all 236 source/configuration
hashes in `work/pty-integration/pr1-source.json` unchanged. Gate log SHA-256:
`c1033fcffdcb93d71608bc5a7f2da22fbc926a3e5ed787c5f331b931e7e84bf9`.

The separate code-quality decision and new remote CI results remain independent
merge requirements; this reviewer has not merged or published anything.
Qualification limits remain: these local synthetic fixtures do not prove live
remote PTY/authentication behavior or long-running resource bounds. Known
missing bare programs are covered; broader operating-system launch failures
retain existing conservative handling. A failed socket unlink may leave a
private stale pathname, while final process evidence remains preserved.
