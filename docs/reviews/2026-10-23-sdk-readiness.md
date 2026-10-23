# Workspace SDK readiness checkpoint

## Ownership and source

The isolated branch `codex/sdk-readiness` starts from GitHub main
`0b7bbc0a9be0cce0713df05be56f2a63d6c14c30`, verified with `git ls-remote`
on 2026-10-23. The original checkout is
`/Users/nessa/Documents/NessaLabs/workspace-sdk`; its uncommitted historical
review-file addition remains untouched. Supported thread retrieval identified
“Design unified local-cloud SDK” as the completed previous owner. PRs #1 and #2
were already merged; this task does not redo them.

## Findings and fixes

- P2: a dangling `state.json` symlink was mistaken for an absent store.
  The regression failed before the fix; metadata inspection now distinguishes
  genuine absence from invalid storage. File-state tests cover links, directories,
  corruption, unknown versions, oversized state, and legacy migration.
- P2: caller cancellation could skip transfer cleanup. The owned workflow
  workers retain admission, workflow leases, sequential cleanup, and final receipt
  publication after observer cancellation. Ten new sync tests exercise the
  failure and cancellation boundaries.
- Transfer boundary hardening rejects invalid cursor, chunk length, declared
  size, and checksum observations before importing.

## Source checkpoints and validation

- Implementation and documentation snapshot: `5dfdccca0bfaef1c04891192fa1009360a120f3d`.
- Strict Clippy caught two test fixture initializers; corrected without suppressions
  in `bf60930a41a6ef3eed425f57635e9ff6bbd9d624`. No assertion was weakened.
- Seven new persistence tests, ten focused tests total, pass with all features
  and without default features. The dangling-symlink test failed before the fix.
- Ten new sync tests pass. The pre-fix gate failed the two initial cancellation
  regressions, retaining the observed leaks. Additional cases cover rejected
  uploads/imports, malformed chunks and archive metadata, exact file bytes,
  canceled admission waiters, held workflow leases, and one continuation launch.
- Full unchanged `RUSTC_WRAPPER= scripts/quality-gate.sh` passed against
  `bf60930a41a6ef3eed425f57635e9ff6bbd9d624`. The wrapper override avoids a
  sandbox-incompatible compiler cache; it does not change Rust code or test gates.
- Nessa Docs built catalog CLI accepted six pages with zero issues. The installed
  renderer displayed all six diagrams in the in-app browser. Root drill-down,
  state details and the execution-to-sync navigation worked. Settings > Health
  reported six accepted documents and no errors or warnings.
- Relative Markdown links and `git diff --check` passed.

The full gate completed with exit zero: 302 successful test executions across
both feature configurations, doctests and explicit performance qualifications.
Five ignored occurrences include the opt-in Boat live test and performance tests
before their explicit runs; live Boat was not run. Managed performance recorded
three samples; PTY performance recorded fifteen samples at populations one/four.
The [durable evidence index](../evidence/2026-10-23-sdk-readiness/index.json)
records original and normalized hashes and exact reproduction commands.

The local workflow example also completed with exit zero:
`RUSTC_WRAPPER= cargo run --locked --no-default-features --example local_workflow -- target/debug/workspace-supervisor`.
It executed a real local process, staged/applied file sync and destroyed its owned
workspace. An existing no-default-feature dead-code warning remains; the required
all-feature Clippy and warning-free Rustdoc gates passed.

Original gate log SHA-256: `0231858ef4948d4666549304f12b37be4280ad848e4165b170625c5a89ad280c`.
Original example log SHA-256: `4ec5634f731fd2d076c5be380b2b94bc8189f0f890a15932399a20a3d0c58a12`.

## Independent review

`final_design_review` and `final_adversarial_review` inspected the actual changes.
Neither authored the implementation. Design review requested explicit direct-transfer
cancellation guidance; it is now in the API Rustdoc and SDK guide. Adversarial review
identified unbounded detached direct transfers; shared admission now retains
backpressure and the queued-transfer regression prevents unseen work.
Both independent reviewers explicitly PASS the final implementation at
`bf60930a41a6ef3eed425f57635e9ff6bbd9d624` and documentation checkpoint
`c39e2d20389c6da86df69c5acd67b5abcc2d6e46`, after inspecting the passing gate
and example logs. No unresolved change-scoped P1/P2 findings remain. Subsequent
evidence-record changes do not alter implementation or assertions.

Draft [PR #3](https://github.com/nessalabs/workspace-sdk/pull/3) is published.
Hosted Linux/macOS CI is pending. CodeRabbit's success status means draft review
was skipped, not an additional code-review pass. Parent owns final exact-head
merge after required CI and independent review. This task does not merge.

## Remaining boundaries

Observer cancellation does not request operation cancellation. Applications must
retain quiescence and runtime ownership through completion. Direct capture/restore
have no durable public operation handle; retain and await an application-owned task.
Runtime shutdown/process death and cleanup transport failures remain outside the
cleanup guarantee. Different service instances still require application
coordination for direct transfers.

Git subprocess output is collected before the manifest-size check. Archive limits
do not establish a global subprocess-memory bound; this preexisting limitation is
not changed by these fixes. Native agent sign-in/inference and live SSH/container
qualification remain separate, unrun acceptance work. Historical Vercel evidence
uses LocalProvider and is not attributed to this source revision. Boat deletion
may remain blocked. Registry distribution and additional bindings remain deferred.

