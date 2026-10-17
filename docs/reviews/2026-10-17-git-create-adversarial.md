# Git worktree creation: independent adversarial acceptance

Decision: **FINAL PASS** for source `affc2ce3de7796cdac269d8c0e19d797045adc51`. No unresolved P1/P2 finding affects this scope. Reviewer: independent adversarial review subagent, separate from implementation and code-quality review.

## Reviewed scope

Read repository instructions and standards, `src/git/{command,create,discovery,mod,porcelain}.rs`, the guide and all eleven `tests/git_worktree_create.rs` cases. Creation composes an existing workspace connection and managed launch claim; discovery retains a compatible alias. There is no new ownership registry, automatic attachment, deletion authority or transaction journal.

Reviewed request validation, literal argv, environment sanitization, strict commit verification, stable qualified launch identity, concurrent exact retries, acknowledgement uncertainty, cancellation, execution deadlines, Git's cleanup behavior and fixture ownership. No source edits, builds or provider resource operations were performed by this reviewer.

## Findings and closure

1. **P2, closed: a hex-shaped revision is not necessarily a full commit pin.** A forty-character value can abbreviate a SHA256 object, a sixty-four-character value can name a SHA1 repository ref, and a tag object can peel to another object. The implementation requires successful, complete, bounded `rev-parse --verify --end-of-options OID^{commit}` output containing exactly the requested full commit ID, case-normalized. Both verification and creation disable replacement objects. Real repository tests exercise all three rejection cases and demonstrate replacement metadata that remains effective for ordinary Git while SDK checkout uses the original pinned tree.
2. **P2, closed by explicit contract: Git itself may clean an in-progress checkout.** Absence of SDK-issued rollback does not imply Git never removes files. The guide and API now document Git's failure/signal cleanup, possible partial branch/files, trusted hooks and filters, stable repository requirements and exclusive quiescent destination access. They do not promise transactional creation, atomic verification-to-execution binding, or protection from external writers.
3. **P2, closed: numeric PID fixture cleanup could signal a reused process group.** The draft read `process.json` and retained a raw PID after the process could exit. All raw PID signalling and its cleanup guard were removed. Cleanup now targets qualified native run references through the actual backend, not OS process numbers.
4. **P2, closed: acknowledgement-only tracking and unguarded short cases missed uncertain launch windows.** The code-quality reviewer found this after my earlier source PASS; that earlier review missed the pre-acknowledgement and short-case gaps. Re-reviewed the corrected full file: every case retains an outer fixture; intended qualified native identities and inner backends are deduplicated before awaiting start. An unexpected returned identity is not targeted and marks uncertainty. Cleanup bypasses the simulated observation seam, awaits cancellation and independently requires a real terminal state. `Lost` is not terminal. Missing/error/timeout/uncertain cleanup preserves the control directory and fails visibly. These preservation branches were source-reviewed, not separately fault-injected in this increment.

The final source review found no further actionable P1/P2 issues. Strict short-branch validation, bounded path/deadline/retention inputs, exact workspace matching and explicit executable selection remain enforced. Identical retries have no mutable destination/branch inventory preflight that would reject a previously successful checkout.

## Actual acceptance evidence

Inspected the successful [full cloud run 39078477444](https://github.com/nessalabs/workspace-sdk/actions/runs/39078477444), its downloaded metadata and raw log, and the local checkout at the exact source above. Both Ubuntu 24.04 and macOS 15 jobs checked out that source and completed `scripts/quality-gate.sh` successfully. Formatting, warning-denying all-target/all-feature Clippy, both feature selections, examples, warning-denying docs and managed/PTY performance qualification passed.

Counted each of the nine new private unit tests and eleven new integration cases passing twice per OS, once under each feature selection: **80 new-test executions**. The existing actual native `killed_supervisor_is_lost_and_same_id_never_relaunches` coverage also passed, separately from the new simulated-Lost case. Existing discovery and the full retained suites ran in the same gate.

Verified the [qualification evidence](../evidence/git-worktree-create-2026-10-17/qualification.json) against actual job metadata, raw-log SHA256 `f30a54279af7a154f787d606743dfa521c63cdfecbac93aff9e3dd1c0680da9b`, and extracted performance JSON. Each OS has three managed samples and fifteen PTY samples; the saved objects match those embedded in the raw logs. These debug-build samples describe the recorded Linux x86_64 and macOS aarch64 environments; they establish no statistical regression baseline or creation speedup.

## Evidence boundaries and remaining tradeoffs

- The lost-reply seam loses acknowledgement after a real native `start` returns; retry and an invocation counter establish reuse of one actual creation claim. This is not a physical crash or power-loss experiment.
- The new Lost case simulates an uncertain backend observation over a real, initially active native claim. It does not kill a supervisor. Existing native supervisor-loss coverage is separate evidence, not a new Git crash test.
- Cancellation and execution timeout use a controlled wrapper that waits before running Git creation. They qualify managed lifecycle and no-relaunch behavior, not cancellation during a partially populated Git checkout.
- No executed old-source failing probe or mutation experiment is claimed. Draft defects were identified by source/design review; final tests passed in the actual gate.
- Commit verification repeats before every `start_create` and may fail if repository/executable/object access disappears, including on a retry of an existing claim. Direct `anchor.run(saved_id)` remains the inspection path without re-verification. Verification's output acceptance limit is checked after backend capture/allocation limits; Git may fetch promised objects or write configured diagnostics before the creation claim.
- Repository namespace and destination stability remain caller obligations. Hooks, filters, configuration and executable are trusted host code. Git may reuse an empty directory, leave partial state or clean an in-progress worktree. There is no SDK force/reset/rollback, automatic attachment or automatic cleanup permission.
- Qualification uses disposable native local repositories. It makes no live remote-provider, offsite durability, external-writer safety or automatic failover claim. Custom backends must honor the existing managed launch contract.
