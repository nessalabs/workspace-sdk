# Artifact traversal quality review

- Reviewer: independent quality/design agent `/root/boat_cleanup_quality`.
- Reviewed source: `539d95c6dc1fea6f2e4a453ceb5541c12ae06bb9`.
- Scope: `src/checkpoint/infrastructure/filesystem.rs`; streaming artifact traversal and three bound regressions. No other production change is covered by this increment.
- Final decision: **PASS**. No unresolved P1/P2 quality/design findings.

## Design assessment

The production change consumes `ReadDir` directly instead of collecting and sorting every child. Publication still syncs each child before its parent and completes traversal before rename. Sibling ordering is not part of the durability contract. Shared entry/byte budgets, depth rejection, symlink/special-file checks, safe sync opens, and error propagation remain intact. Infrastructure retains filesystem responsibility; no interface, domain model, or dependency direction changes were introduced.

Streaming removes the explicit allocation proportional to one directory's width and its sorting work. Recursive directory iterators remain open during descent, bounded by the existing depth limit. This is a source-level assessment, not an allocation or throughput measurement.

## Findings and disposition

| Severity | Finding | Disposition |
| --- | --- | --- |
| P2 | Initial entry/byte regressions used `max_files: 0`, which public `with_limits` rejects. They bypassed the supported positive configuration and removed the per-file allowance term. | Fixed before the frozen source: both use `max_files: 1`. The entry fixture has 5,121 empty files against a 5,120 allowance. Two 2,052-byte files each fit the 4,102-byte allowance, while their 4,104-byte aggregate exceeds it. Source re-review and actual test execution pass. |

The new tests inspect real directory entries and metadata while using the existing callback seam to observe whether rename is attempted. They verify the expected bound error, no rename, retained stage, and unchanged previous artifact. That previous artifact is an unrelated fixture: the primary safety evidence is the explicit refusal to invoke rename, not a simulated overwrite of a prior checkpoint. The aggregate-byte test is independent of directory iteration order. Existing ordering tests assert child-before-parent relationships rather than sorted siblings; their assertions were retained.

## Actual verification evidence

Inspected `git show` for the exact commit, `scripts/quality-gate.sh`, and the locally saved JSON/log for [run 39073806361](https://github.com/nessalabs/workspace-sdk/actions/runs/39073806361). Both checkout logs identify the exact reviewed SHA. Both `quality (ubuntu-24.04)` and `quality (macos-15)` completed `scripts/quality-gate.sh` successfully.

The script includes format, Clippy with warnings denied, feature-selected builds/tests, rustdoc with warnings denied, supervisor build, and explicit managed/PTY performance qualification. The actual logs show all nine filesystem publication tests passing twice per OS, under all features and no default features: 18 executions per OS. This includes the three new entry/aggregate-byte/depth regressions, the existing child-before-parent publication-order test, failure boundaries, and symlink/special-file rejection. Review commands only read source and evidence; the reviewer did not run builds or tests locally.

Evidence files inspected beneath the task's `work` directory:

- `workspace-cloud-run-39073806361.json`
- `workspace-cloud-run-39073806361.log`
- `workspace-artifact-stream-performance-ubuntu-24.04.json`
- `workspace-artifact-stream-performance-macos-15.json`

Also checked `docs/evidence/artifact-stream-2026-10-17/qualification.json` against those original files: both job arrays and conclusions match; both OS performance arrays match completely; and the original log's SHA-256 matches the recorded `db5483f04055c6252a71222b215c231a826f031aa22a51d4d2ba904cd958f332`. Its source SHA matches the actual checkout logs. The evidence summary is consistent with the reviewed originals.

Both performance artifacts contain three managed samples and fifteen PTY samples with debug assertions enabled. Linux used x86_64; macOS used aarch64.

| Environment | Managed cancellation, ms | PTY cancellation, ms | Retained managed / PTY bytes |
| --- | --- | --- | --- |
| Ubuntu 24.04 | 51.83–51.97 | 51.74–57.35 | 32,768 / 131,072 |
| macOS 15 | 73.22–123.72 | 71.28–284.50 | 32,768 / 131,072 |

Retention values were consistent across each artifact's samples. These are qualification observations, not a statistical regression baseline or a checkpoint traversal benchmark.

## Remaining limits

The callback tests establish traversal orchestration and publication refusal at bounds; existing native sync tests establish syscall success on the CI filesystems. Neither proves power-loss persistence or a storage controller's durability behavior. No directory-width memory benchmark was added, so no measured allocation reduction or throughput improvement is claimed. This independent design decision does not substitute for the separate adversarial review required by `AGENTS.md`.
