# Managed Git worktree creation quality review

- Reviewer: independent quality/design agent `/root/boat_cleanup_quality`.
- Exact source: `affc2ce3de7796cdac269d8c0e19d797045adc51`.
- Scope: shared Git command construction, managed creation request/service, discovery compatibility alias, private validation reuse, SDK guide, and eleven native fixture cases.
- Final decision: **PASS**. No unresolved P1/P2 quality/design findings in this scope.

## Findings, fixes, and re-review

| Severity | Finding | Final disposition |
| --- | --- | --- |
| P2 | A 40/64-character hexadecimal shape alone does not establish an exact commit pin: a SHA-256 abbreviation, a hex-named ref in a SHA-1 repository, or an annotated tag can resolve differently. Replacement refs can also alter the observed tree. | Fixed: bounded `rev-parse --verify --end-of-options OID^{commit}` must return exactly the requested full ID, compared to its lowercase form. Both verification and creation disable replacement objects. Three real-repository rejection cases and a replacement-ref case pass. Verification is explicitly a separate, non-atomic preclaim operation, not a transactional repository reservation. |
| P2 | A blanket no-cleanup/no-deletion promise would contradict Git's own failure/signal cleanup of an in-progress checkout, particularly with an externally reused destination. | Fixed contract: caller must maintain a stable repository and exclusive, quiescent destination. Git may reuse an empty directory, remove an in-progress worktree, or leave partial files/branch state. The SDK issues no additional rollback/reset/force/deletion and grants no ownership or automatic attachment. |
| P2 | Draft test cleanup used cached numeric process/group IDs and actual supervisor killing, risking stale-target signaling and weak fixture lifetime guarantees. | Fixed: no raw PID signaling remains in the new test file. Cleanup uses workspace-qualified native run identities. The new Lost case explicitly simulates an observation over a real active native claim; it does not claim to kill a supervisor. Existing provider-level actual supervisor-loss coverage remains separate. |
| P2 | A draft fixture recorded a native claim only after awaiting start, leaving a watchdog/unknown-acknowledgement window. Several shorter cases also lacked the outer ownership guard. | Fixed: every case retains an `Arc<Fixture>` under the outer watchdog. Intended qualified run identity and inner backend are deduplicated before awaiting start. Returned identity mismatch is uncertainty, not authority to cancel an unknown target. Cleanup bypasses observation seams, awaits cancellation, separately inspects native terminal state, and preserves fixture storage on missing/error/timeout/unexpected identity. `RunState::is_terminal` excludes Lost. |

All fixes received source re-review before the exact-source cloud gate. The final scoped files match the reviewed working files. Fixture-preservation branches for every unexpected identity, missing state, and cleanup failure were source-reviewed; they were not all fault-injected in the passing run. No executed old-source red probe or mutation test is claimed; initial defect findings came from source/design review.

## Design assessment

One concrete `GitWorktrees` binding retains the existing workspace and explicit Git executable. The compatible `GitWorktreeDiscovery` alias preserves existing `new`/`discover` calls. Shared command construction centralizes inherited Git environment handling without a second provider, registry, scheduler, or ownership model. Mutation validation and launch construction remain cohesive in the private creation module; tests use existing backend ports rather than a production-only abstraction.

The request carries a caller-persisted qualified `RunRef`, explicit POSIX target-host path, new branch reference, full object ID, deadline, and retained-output budget. Validation rejects ambiguous path components, invalid branch restrictions, wrong-workspace identity, invalid object-ID shape, and invalid bounds before creation. Shell-free argv uses `worktree add --no-track -b` and `--`; it never introduces force/reset options. The existing `Workspace::start_with` owns the fingerprinted creation launch and returns the ordinary `Run`.

Exact commit verification requests ten seconds and accepts at most 256 KiB combined output after backend capture. It requires successful complete execution and exactly one matching full-ID line; captured Git stderr is omitted from its fixed diagnostic. Host configuration, hooks, filters, and safe-directory checks remain trusted inputs. Verification may fetch promised objects or write configured diagnostics outside the creation claim. It repeats before each call, so a retry can fail when the repository/executable becomes unavailable; `anchor.run(id)` remains the inspection/reconnect path without verification. Repository identity is not frozen between verification and creation. These limitations are documented rather than hidden behind an exactly-once claim.

## Test assessment and actual execution

Nine new private creation tests cover request/serialization bounds, deterministic argv/environment, exact commit verification, replacement disabling, and incomplete/failed result handling. Eleven native cases use real disposable SHA-1/SHA-256 repositories and LocalProvider/native managed runs. They cover ambiguous pins, replacement refs, dirty-anchor preservation, paths with spaces/newlines, explicit discovery/attachment, incompatible request reuse, Git conflict refusal, concurrent identical requests, lost start reply, cancellation/timeout, and simulated Lost observation.

The lost-reply forwarding seam drops only the acknowledgement after a real native start; the wrapper counts only `git worktree add`, not commit verification. Fixture repository-local hook/signing settings also apply to SDK-executed Git commands. All cases use the guarded fixture lifetime, including SHA-256 rejection. Intended claims are registered before effects and retained independently of the watched task.

Cancellation and execution-timeout cases hold a controlled wrapper **before Git starts**, so they establish managed launch ownership and no replay, not Git partial-checkout cancellation behavior. The new Lost case observes a real active process before enabling its explicitly simulated Lost result, then cleanup uses the underlying native backend. Actual supervisor loss is separately exercised by the existing `killed_supervisor_is_lost_and_same_id_never_relaunches` test, whose passing executions appear under both feature selections and operating systems in this run.

Inspected the exact commit with `git show`, compared scoped source against it, and read original run metadata/logs and performance JSONs. [Run 39078477444](https://github.com/nessalabs/workspace-sdk/actions/runs/39078477444) completed successfully on Ubuntu 24.04 and macOS 15. Both checkout logs identify the exact reviewed SHA; both jobs passed `scripts/quality-gate.sh`.

The full script runs formatting, Clippy with warnings denied, feature-selected builds/tests, rustdoc with warnings denied, supervisor build, and explicit managed/PTY performance qualification. Individual log names confirm 18 new creation-unit executions and 22 new native-case executions per OS: nine and eleven tests respectively, each under all features and no default features, totaling 80 new test executions. All pass. The saved `workspace-git-create-test-summary.json` counts agree with those original log observations. Literal escaped ANSI markers in the raw log do not change the individual result lines used for this review. The reviewer performed read-only inspection and document validation locally, with no local builds or test runs.

Original evidence inspected beneath the task's `work` directory:

- `workspace-cloud-run-39078477444.json`
- `workspace-cloud-run-39078477444.log`
- `workspace-git-create-test-summary.json`
- `workspace-git-create-performance-ubuntu-24.04.json`
- `workspace-git-create-performance-macos-15.json`

Checked `docs/evidence/git-worktree-create-2026-10-17/qualification.json` against the originals: job arrays and conclusions match exactly; both full performance arrays match; source SHA matches both actual checkout logs; and the original log's SHA-256 matches recorded `f30a54279af7a154f787d606743dfa521c63cdfecbac93aff9e3dd1c0680da9b`. Its named-test counts, four P2 closures, simulated-Lost/pre-Git cancellation distinctions, and stated evidence limits agree with this review.

Both performance artifacts contain three managed samples and fifteen PTY samples with debug assertions enabled. Linux used x86_64; macOS used aarch64.

| Environment | Managed cancellation, ms | PTY cancellation, ms | Retained managed / PTY bytes |
| --- | --- | --- | --- |
| Ubuntu 24.04 | 52.45–52.51 | 52.27–160.88 | 32,768 / 131,072 |
| macOS 15 | 58.94–152.49 | 59.73–225.78 | 32,768 / 131,072 |

Retention values are consistent across samples. These qualify existing runtime behavior; they are not a statistical regression baseline or a Git worktree creation latency/throughput benchmark.

## Remaining limitations

This is one managed creation invocation, not transactional worktree lifecycle or generic provider exactly-once execution. Local native evidence does not establish SSH/Boat transport behavior or arbitrary custom backend contracts. Commit verification has separate ancillary effects and availability requirements and does not freeze repository namespaces. Trusted host Git behavior and concurrent external mutation remain caller responsibilities. No automatic cleanup/rollback, ownership registry, or physical persistence guarantee is introduced. This quality decision does not replace the independent adversarial review required by `AGENTS.md`.
