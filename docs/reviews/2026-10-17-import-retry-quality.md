# Tree import retry quality review

- Reviewer: independent quality/design agent `/root/boat_cleanup_quality`.
- Exact source: `e6fbc1a8dbccb62aeb5f4efd136eae2b69a82877`.
- Scope: `filesystem.rs` ancestor-helper visibility; `remote_import.rs` retry/receipt syncing; `remote_storage.rs` concrete syscall seam; `remote_tree.rs` contract documentation; nine tests in `remote_import_tests.rs`.
- Final decision: **PASS**. No unresolved P1/P2 quality/design findings in this scope.

## Findings, fixes, and re-review

| Severity | Finding | Final disposition |
| --- | --- | --- |
| P2 | Matching Pending files skipped `publish_file`, including its failed parent sync. | Fixed: validated selected regular files and parent directories are synced before completion. Repeated fault tests require errors while the boundary remains unavailable, then successful retry. |
| P2 | The Complete fast path validated bytes but immediately acknowledged without resyncing files or a potentially uncertain Complete receipt. | Fixed: Complete retries sync validated selected files, the existing receipt, and their ancestry before acknowledging. File, receipt, and post-persist directory fault tests require an error while the corresponding sync remains unavailable. |
| P2 | An existing `.workspace-sdk` directory could have survived an interrupted creation without its directory entry being durable. Syncing only that directory when saving Pending did not fulfill the existing durable-intent-before-publication contract. | Fixed: `save_receipt` finishes canonical metadata-directory ancestry through filesystem root after atomic receipt publication. The new prepublication test injects the ancestor error twice while the visible receipt is Pending and target is absent, verifies no selected-file publication, then retries successfully. |
| P2 | A draft helper collected an owned `PathBuf` for every selected path prefix. Accepted deep paths could expand working memory far beyond the selected tree's representation. | Fixed: direct per-entry ancestor walks retain one prefix at a time instead of accumulating all prefixes. Shared parents may be synced repeatedly. No new collection, public bound, interface, or parallel publication framework was added. |

Draft-test review also corrected ordering assumptions and coverage before execution. Changed-Pending fault injection now requires an existing matching target and Pending receipt and asserts injection occurred. Matching-retry ordering searches for a root sync after the selected nested-directory sync, allowing earlier correct intent-root syncs. Complete and empty-tree retries verify ordered ancestry through filesystem root rather than stopping at project root. These review corrections are resolved and are not additional outstanding findings.

All fixes received source re-review before the final cloud gate. The final committed source matches the reviewed working files; a comparison against the five scoped files showed no difference.

## Design and test assessment

The implementation keeps validation, receipt identity, and import sequencing in `remote_import`; filesystem syscall behavior remains in the existing `PublicationStep`. `atomic_bytes_with` supplies a narrow concrete callback seam, while the existing export wrapper retains its prior immediate-parent sync scope. No public identity, receipt version, overwrite permission, retry selection, lifecycle ownership, or external port changes are introduced.

Matching files are validated before retry effects. Regular files use safe file syncing; symlinks sync their containing directories without opening their targets. File-parent chains remain bottom-up, and empty trees still sync project-root ancestry. Conflicting edits produce errors before retry sync or overwrite. The caller's existing stopped-writer requirement remains essential.

The nine tests use real temporary filesystem entries, receipt persistence, and production syscall steps. Their callbacks execute each real step except at the explicitly injected error boundary. The Complete receipt test observes a receipt actually made visible by atomic persist before refusing its directory-sync step; it does not manufacture a Complete journal. Pending failures retain published payloads or prevent publication according to the tested boundary. Additional cases cover selected-file and receipt-sync errors, retry ordering, empty trees, dangling symlinks, and edited Pending data. These are injected errors, not observed kernel/storage failures.

Tests were drafted before implementation, but no executed old-code failing probe was performed or claimed. The original defect assessment is based on static control-flow review; final behavior has the actual execution evidence below.

## Actual verification evidence

Inspected the exact commit with `git show`, compared scoped source files against it, read the cloud run JSON/log, and inspected both original performance JSONs. [Run 39076072324](https://github.com/nessalabs/workspace-sdk/actions/runs/39076072324) completed successfully on Ubuntu 24.04 and macOS 15. Both checkout logs name the exact reviewed SHA; both jobs successfully completed `scripts/quality-gate.sh`.

The script runs formatting, Clippy with warnings denied, feature-selected builds/tests, rustdoc with warnings denied, supervisor build, and explicit managed/PTY performance qualification. All nine `checkpoint::infrastructure::remote_import::tests` names appear as passing under both all-features and no-default-features selections on each OS: 18 executions per OS, 36 total. The reviewer ran only read-only source/evidence inspection commands and document validation locally; no local build or test execution was performed.

Original evidence inspected beneath the task's `work` directory:

- `workspace-cloud-run-39076072324.json`
- `workspace-cloud-run-39076072324.log`
- `workspace-import-retry-performance-ubuntu-24.04.json`
- `workspace-import-retry-performance-macos-15.json`

Checked `docs/evidence/import-retry-durability-2026-10-17/qualification.json` against those originals: job arrays and conclusions match exactly, both OS performance arrays match completely, and the original log's SHA-256 matches recorded `417cfff5a722ad891f7a789575df2f206ce209b23261cae2992ffb8cc24e37ba`. Its source SHA and four P2 closures agree with this review and the actual checkout logs. The evidence summary accurately distinguishes injected syscall-boundary failures, local qualification, and remaining limitations.

Both performance artifacts contain three managed samples and fifteen PTY samples with debug assertions enabled. Linux used x86_64; macOS used aarch64.

| Environment | Managed cancellation, ms | PTY cancellation, ms | Retained managed / PTY bytes |
| --- | --- | --- | --- |
| Ubuntu 24.04 | 52.16–53.16 | 52.11–103.97 | 32,768 / 131,072 |
| macOS 15 | 80.64–129.32 | 57.10–149.44 | 32,768 / 131,072 |

Retention values are consistent across their respective samples. These qualify existing runtime behavior; they are not a statistical regression baseline or an import latency, memory, or throughput benchmark.

## Remaining limitations

Retry validation and syncing add work proportional to selected files and their parent chains, including repeated shared-directory syncs. No measured import performance improvement is claimed. Filesystem/controller persistence behavior and physical power loss remain unproved. Canonical ancestry syncing and safe final-component opens do not create a race-proof filesystem sandbox against concurrent hostile mutation; project writers must remain stopped. No live remote provider qualification follows from these local native tests. This independent quality decision does not replace the separate adversarial review required by `AGENTS.md`.
