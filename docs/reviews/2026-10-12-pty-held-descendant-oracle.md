# Held-terminal drain oracle correction — 2026-10-12

Independent test author: `code_quality_gate`. Scope is only the held-descendant
case in `tests/runtime_pty.rs`; no SDK production or separately owned PTY source
was edited for this correction.

The retained failure log `work/pty-integration/acceptance-green01.log` records
three passing cases and one failure in shared `Transcript::complete` at its
`!page.gap` assertion. It remains unchanged. The implementation had begun exposing
an incomplete drain honestly; the held-descendant test incorrectly required EOF.

Independently read the exact pinned upstream revision
`c68837e2b7bf1c9009e1a4587a1a58107ec0425c` with `git show`:

- `crates/domain/src/process/outcome.rs` defines `DrainOutcome::Truncated` as
  output cut off by a drain deadline or owner shutdown. Its default drain bound
  is 250 ms, separate from child-exit observation.
- `crates/infrastructure/src/process/lifecycle.rs` stops the reader when that
  bound elapses after leader exit. It requests guardian `Release` after the
  reader is done.
- `helpers/guardian/src/guardian.rs` reports natural `WorkloadExit`; `Release`
  enables cleanup. Natural retirement can happen when the session is empty,
  but this fixture deliberately keeps a live descendant holding the terminal.

Thus this case must retain `gap=true` at completion. The correction uses its own
bounded read loop and checks exact retained text, byte-absolute monotonic offsets,
empty stderr, final `gap=true`, and stable completion/cursors with the same gap
through the original host, a reconnected host, and replay from zero. At published
completion it immediately verifies the descendant is absent or a non-running
zombie; it no longer gives cleanup a grace period after publication. Leader exit
must be confirmed with code zero. The independent fixture cleanup channel stays
open until these assertions finish, and the descendant escape marker must not
exist.

Shared `Transcript::complete` remains unchanged and still rejects gaps. The
normal 300 KiB burst case retains its EOF/no-extra-gap assertions. This is an
oracle correction based on the public drain contract, not suppression of the
incomplete-drain marker. No builds were run by this agent; the implementing agent
must run affected acceptance tests and record their result before acceptance.
