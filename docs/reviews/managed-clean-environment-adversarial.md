# Managed clean environment: adversarial review

**FINAL ADVERSARIAL PASS for `7ff5c9b0d33af85545f6cb11b5c19c3e959c0700`. No unresolved P1/P2 finding within the explicit managed-pipe child environment scope.** Independent reviewer: `workspace_handoff_adversarial_review`. Other agents authored production/tests; this reviewer ran no builds or probes and changed only this record.

## Source and boundary assessment

The additive `start_piped_with_clean_environment` method uses the existing managed run claim, private transport and pipe owner. Optional backends refuse unsupported clearing; there is no inherited-mode fallback. Launch request literals and prior modes retain their encoding and behavior.

An absolute executable is enforced by Workspace, managed backend and native admission before claim creation. This closes the design concern that a relative program might use a platform default executable search even without PATH. Native command setup clears inherited variables before removals, public values and finally transient secret values. Thus explicit values can reintroduce removed names, and secrets override public duplicates. Clearing applies only to the workload child. The new operation is exhaustively classified for private payload transport.

The clean-only fingerprint separator precedes the existing piped separator. Exact clean retries reconnect; same-ID clean/inherited/ordinary rebinding conflicts. Prior piped encoding has an independent golden calculation. No extra journal or control endpoint is introduced, and input prefix/Unknown/EOF/ownership semantics remain those of managed pipes.

Three real-native cases compare an inherited-piped child against a clean child under a synthetic runner environment, without process-global environment mutation. They verify sentinel/PATH/HOME absence, explicit removals and precedence, synthetic secret redaction from claims/Debug, exact binary input/output, EOF, reconnect, retry and mode conflicts. The third case observes the new operation in private request stdin, simulates an unsupported supervisor with bounded invocation counts and no fallback, then mutates the payload after API/backend validation to exercise native absolute-path refusal. Shared registered authority and claim cleanup cover the custom-runner workspace.

## Actual local evidence and sensitivity

The initial focused command passed 164 tests: 145 library, three clean-environment, four pipe, two byte-output and ten workflow cases. Its outer bounded metadata records exit zero, no stop or cleanup error, owned process group absent and 32.05869 seconds elapsed.

The three parent-owned native mutations each built the mutated supervisor successfully, then exited 101 through the intended runtime assertion:

- Removing clearing exposed the inherited synthetic sentinel.
- Removing clean-mode identity separation allowed a prohibited inherited-piped claim rebind.
- Reversing explicit environment precedence returned the public value instead of the synthetic secret value.

Actual failure logs agree with `clean-env-probes-proof.json`; these are semantic catches, not compile failures. All nine frozen file hashes match after restoration. Manifest SHA-256: `a957480e42b80acf3e7bf9acde8fa788cc27c8dd488e02cb9970ce60d74ea608`.

A rebuilt restored supervisor passed before rerunning all three new native cases and four retained pipe cases, seven passing in total (groups 1.28 and 1.23 seconds; outer command 10.41360 seconds). All-feature/all-target denied-warning Clippy, strict documentation and formatting also passed. Completed bounded metadata records exit zero/no stop/no cleanup error/group absence for each command. The runner's group observation does not itself establish cleanup of detached native workloads; the tests separately inspect registered claims and ownership.

## Decision and limits

**Precloud PASS:** source, local qualification, mutation sensitivity and exact restoration are consistent. No unresolved P1/P2 finding remains. Full remote qualification is pending.

This is environment construction, not an executable/filesystem sandbox or secret-output sanitizer. Absolute launchers, interpreters and the operating system can introduce variables themselves. No PATH/HOME/locale/loader defaults are reconstructed. Supervisor/transport environments and credentials are unaffected. No agent host adapter, application migration, physical failure, arbitrary external-effects exactly-once guarantee or live-provider qualification is established. Existing input ambiguity, retained-output gaps and cleanup responsibilities remain unchanged. Local timings are observations, not a baseline or speedup claim.

## Final exact-source qualification

Independently inspected [full cloud run 39170044183](https://github.com/nessalabs/workspace-sdk/actions/runs/39170044183): Ubuntu 24.04 and macOS 15 jobs and both full `scripts/quality-gate.sh` steps completed successfully at exact commit `7ff5c9b0d33af85545f6cb11b5c19c3e959c0700`. All nine frozen source hashes match both working files and the candidate's Git blobs. Raw cloud log SHA-256: `f11f91f42a1d618b55ab91826e47215793b416c1f05bd3ef7b569af9faa57820`.

The raw log contains four successful executions of each of the three new native cases: two feature selections on each OS, totaling 12 new executions. Full qualification also completed formatting, denied-warning Clippy, strict documentation, builds and retained suites. Four complete performance objects parsed from that log match the saved [performance evidence](../evidence/managed-clean-environment-2026-10-18/performance.json): three managed and 15 PTY samples per OS. These debug-build observations are retained qualification data, not a clean-environment performance baseline or speedup claim.

**FINAL PASS.** The exact-source remote result closes the earlier precloud condition. All source contract limits above remain part of acceptance; no broader environment sandbox, adapter or live-provider claim follows from this gate.

## Subsequent documentation-only scope

Reviewed the added AGENTS external-learning rule and [credited LayerFS study](../research/layerfs-learning.md). Independently opened the pinned upstream LICENSE at `16e622e27278396c2cb822b0b0cc75ad03a7102c`: MIT, copyright 2026 Ephemeral-AI, with notice preservation required for copies or substantial portions. The study credits upstream, distinguishes preexisting SDK behavior from the new unimplemented inspection proposal and states that no upstream implementation was imported. It does not turn an MIT finding into a universal legality or unrelated-material license claim. The rule appropriately requires revision-specific license review and credit without claiming every non-MIT license prohibits learning or reuse.

Pinned upstream preview storage documentation explicitly excludes crash/power-loss durability; the replacement core guide separately identifies its disposable profile and incomplete integrated qualification. Actual Commit/Close sources support the study's separate publication/teardown and retained-failure-ownership observations. These are credited design lessons, not SDK qualification evidence.

**Documentation scope PASS.** All nine frozen feature hashes still match. This prose-only extension adds no dependency or Rust behavior; the earlier exact-source cloud gate remains the Rust qualification, with no new test execution claimed for these documentation additions.
