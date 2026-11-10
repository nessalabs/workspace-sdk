# Managed Git worktree removal quality review

- Reviewer: independent quality/design agent `/root/boat_cleanup_quality`.
- Exact source: `17f28bb6893e4de0186d5e393d0c366ad41cf183`.
- Scope: explicit managed removal, shared private request validation, creation/discovery documentation and exports, extracted native fixture, retained creation tests, and twelve removal cases.
- Final decision: **PASS**. No unresolved P1/P2 quality/design findings in this scope.

## Findings, fixes, and re-review

| Severity | Finding | Final disposition |
| --- | --- | --- |
| P2 | Removing the execution anchor, its control storage, or an ancestor could invalidate later run inspection, cancellation and retry. The supervisor checks that its project root still exists for subsequent operations. | Fixed before the final gate: one descriptor snapshot supplies qualified identity, root and control paths. Pure checks reject malformed/non-normalized descriptor paths and removal equal to or containing either protected path at component boundaries. The caller must additionally keep anchor/control/common Git metadata physically outside the removal subtree, including aliases. Unit tests cover prefix lookalikes; native guard tests prove no launch claim or wrapper invocation is created. |
| P2 | Non-force removal is not a general local-data preservation guarantee: ignored artifacts can be deleted, and host configuration can hide ordinary untracked data. | Fixed contract and command: explicit `-c status.showUntrackedFiles=all`, real ordinary-untracked refusal with the repository setting otherwise hiding those files, and an explicit ignored-file deletion case. Guide and API documentation disclose ignored deletion and partial failure; caller authorization, stopped users/processes, and exclusive quiescent access are prerequisites. No transactional guarantee is introduced. |

Both findings received source re-review before the cloud gate. Minor wording was also corrected: pure validation/guards can reject before the native claim; incompatible reuse reaching an existing claim conflicts. A native assertion additionally proves that a successful creation claim cannot be reused to authorize removal. There was no executed old-source red probe or mutation test; findings arose from source/design assessment.

## Design and contract assessment

`start_remove` validates the caller-persisted request, checks the exact workspace identity against one descriptor snapshot, applies lexical root/control guards, and directly delegates a deterministic `LaunchRequest` to existing `Workspace::start_with`. There is no mutable inventory or existence preflight that would reject a legitimate terminal retry or authorize a second deletion after path reuse. The existing managed claim owns execution; the returned ordinary `Run` supplies state, wait, output and cancellation.

The fixed shell-free command is `git --no-pager -c status.showUntrackedFiles=all worktree remove -- PATH`. It adds no force, unlock, prune, reset, or branch-deletion option. Shared environment construction retains ordinary Git safeguards and trusted host configuration, with the documented untracked-file setting override. Private path/bounds helpers remove actual duplication and preserve creation's prior validation predicates and diagnostic text. The service does not add a registry, provider, receipt framework, scheduler, automatic cleanup, or `Workspace::destroy` permission.

Bounds remain explicit: normalized absolute POSIX target-host locator up to 16 KiB, execution deadline 1–3600 seconds, and combined managed output retention 2 bytes–16 MiB, defaulting to 120 seconds and 256 KiB. Pure lexical checks do not establish physical path identity, detect symlink aliases, freeze repository metadata, or infer active processes from paths. Those limitations and caller responsibilities are documented. A wait timeout does not cancel; transport errors and Lost remain uncertain, requiring the original request/identity rather than a fresh ID.

## Test quality and actual execution

Four new unit tests cover request defaults/serialization, revalidated mutable/deserialized bounds, exact/component-boundary descriptor guards and malformed descriptors, and literal non-force argv/environment. Twelve new Local/native cases exercise clean removal with a space/newline path and retained branch; successful retry after replacement checkout reuse; changed path/deadline/output, wrong workspace and create/remove identity conflict; tracked/untracked refusal; locked/main/unregistered/submodule refusal; ignored-file deletion; pure anchor/control rejection before launch; concurrent identical requests; actual lost start acknowledgement; pre-Git cancellation/timeout; and simulated Lost observation.

The main-worktree test uses a surviving linked anchor so it reaches Git's main-worktree refusal rather than the SDK's same-anchor guard. Guard cases assert no native claim and no wrapper invocation, not merely an error. The wrapper recognizes adjacent literal `worktree`/operation arguments despite the added `-c` options and delegates real Git operations. Successful/failed terminal retries verify their prior outcomes and preserve a replacement checkout or a subsequently cleaned checkout instead of executing again.

The shared fixture retains intended qualified native identities before awaiting start, including unknown acknowledgement. Every case retains source/control storage under an outer watchdog. Cleanup uses the inner backend, separately observes native terminal state after cancellation, and preserves storage on uncertainty or failure; no numeric PID signaling is introduced. These failure-retention branches were source-reviewed, not all fault-injected during the passing run. Fixture-local hook, signing, fsmonitor, attributes, excludes, untracked-cache and line-ending settings isolate test repositories from host preferences without changing production policy.

Cancellation/timeout hold a controlled wrapper before Git removal begins; they do not establish rollback or partial-deletion behavior during actual Git removal. The Lost case explicitly simulates an observation over a real active native claim. Actual supervisor-loss qualification remains separate existing provider coverage.

Inspected `git show` for the exact commit and compared scoped source against it: no difference from reviewed working files. Read the original metadata/log for [run 39080090282](https://github.com/nessalabs/workspace-sdk/actions/runs/39080090282). Both Ubuntu 24.04 and macOS 15 checkout logs identify the exact reviewed SHA; both complete `scripts/quality-gate.sh` successfully. The gate includes formatting, Clippy with warnings denied, feature-selected builds/tests, rustdoc with warnings denied, supervisor build, and explicit managed/PTY qualification. Reviewer commands only inspected source/evidence and validated this document; no local builds/tests were run.

Individual passing names in the original log establish, per OS:

- Four removal unit tests twice: eight executions.
- Twelve removal native cases twice: 24 executions.
- Nine retained creation unit tests twice: 18 executions.
- All eleven retained creation native cases twice: 22 executions.

Thus all 64 new removal executions pass across both feature selections and operating systems, and all 44 retained creation native executions pass after shared-fixture extraction. The evidence summary's named counts match independent counts derived from the actual test sources and raw log.

## Evidence consistency and performance

Checked `docs/evidence/git-worktree-remove-2026-10-17/qualification.json` against original `../workspace-cloud-run-39080090282.json` and `.log`: job arrays match exactly, both jobs and the run conclude success, and both checkout SHAs match the recorded source. The original log's SHA-256 matches recorded `487708ea91e27f0460f302311e24a350f1aa78cad537e16db354bd9fec4c5d92`.

Independently parsed the complete performance JSON objects embedded in the raw log, removing only the runner/step/timestamp prefix. Both objects per OS match the evidence summary's full performance arrays exactly. Each OS has three managed and fifteen PTY samples with debug assertions enabled; Linux used x86_64 and macOS aarch64.

| Environment | Managed cancellation, ms | PTY cancellation, ms | Retained managed / PTY bytes |
| --- | --- | --- | --- |
| Ubuntu 24.04 | 52.47–95.21 | 52.25–54.84 | 32,768 / 131,072 |
| macOS 15 | 91.70–192.27 | 72.65–234.34 | 32,768 / 131,072 |

Retention values are consistent across their respective samples. These qualify existing runtime behavior, not removal latency/throughput or a statistical regression baseline.

## Remaining limitations

This is an explicitly authorized managed Git command, not transactional deletion or ownership inferred from discovery/creation. Ignored files can be deleted and errors/signals can leave partial mutations. Caller exclusivity and stable physical separation must hold through resolution of uncertainty; lexical guards cannot enforce aliases or external concurrency. Evidence is Local/native Linux/macOS only, with no live SSH/Boat/Windows or arbitrary custom-backend exactly-once claim. No physical persistence/power-loss test or Git removal performance benchmark was performed. This independent quality decision does not replace the separate adversarial review required by `AGENTS.md`.
