# Existing linked-worktree return: adversarial acceptance

Decision: **FINAL PASS** for test-only source `022f8859be2d65fe741a4da7d2b2419ea1e1f2ef`. No unresolved P1/P2 finding affects this qualification. Reviewer: adversarial subagent, separate from test implementation and code-quality review. This increment adds three tests and helpers; it changes no production behavior or public API.

## Reviewed flow and oracles

Read repository instructions and standards, the actual `tests/checkpoint_engine.rs` diff and existing checkpoint/target selection boundaries. The tests use a real linked Git checkout, deliberately stage contents different from working-tree contents, then snapshot metadata after staging. The regular-file byte oracle includes the main checkout, common repository objects/refs/logs and both worktree indexes; the linked `.git` pointer and excluded local data are also checked. No subsequent Git status/diff command refreshes the index and contaminates that oracle.

The selected-file path is meaningful: capture working-tree baseline, export its selected archive, copy bytes into a disposable target transfer path, import with the actual selected-path receipt, edit files, then use receipt-aware target export and `capture_archive` before reconciliation. A plain restored directory recapture would lack original Git tracking information and would not establish tracked-but-ignored return selection; these tests avoid that false-positive route.

Three cases qualify:

- Clean return retains a tracked-but-ignored edit and tracked deletion, remote addition and disjoint local addition; reconciliation leaves the source untouched; explicit apply updates working-tree content. Exact apply retry after a later user edit returns the same receipt and preserves that edit. Explicit rollback restores preapply regular-file bytes. Git metadata and excluded local data remain unchanged throughout.
- Nonquiescent apply and apply after a selected destination edit refuse, leave source bytes untouched and remain `NotStarted`.
- Divergent local/target edits retain exact base/local/remote conflict originals. Unresolved apply and empty resolution acknowledgement refuse without changing source or Git metadata and remain `NotStarted`.

No actionable P1/P2 issue was found in source review. These are public-flow assertions, not implementation-mirroring mocks. Generated checkpoint identities are not mistaken for deterministic content identities. The regular-file snapshot oracle deliberately does not claim directory-attribute or symlink coverage.

## Actual verification

Inspected [full cloud run 39087185162](https://github.com/nessalabs/workspace-sdk/actions/runs/39087185162), downloaded job metadata, raw checkout/test logs and the exact local source. Ubuntu 24.04 and macOS 15 both checked out the source above and successfully completed `scripts/quality-gate.sh`. Formatting, warning-denying all-target/all-feature Clippy, both feature selections, examples, warning-denying documentation and managed/PTY qualification passed.

Independently counted all three new named tests passing twice per OS, once per feature selection: **12 new-test executions**. The retained healthy/broken linked capture, ignore/tracked selection, checkpoint, native/provider and recovery suites ran in the full gate. No old-source RED or mutation probe was performed or required to claim this existing-behavior test qualification. This reviewer ran no builds/tests and edited only this review record during acceptance.

Verified the [qualification evidence](../evidence/linked-worktree-return-2026-10-17/qualification.json) against actual run metadata and raw-log SHA256 `26501cc366a0a8ad065535f30e9df66972c0725c72d1fe06d6b83452236ca1c2`. Every full three-managed/fifteen-PTY sample object per OS matches the raw log. Those hosted debug-build Linux x86_64 and macOS aarch64 runtime samples are not a statistical regression baseline or linked-return throughput benchmark. No evidence discrepancy was found.

## Limits

- Qualification uses actual local Git/filesystem helpers and explicit archive copying. It does not exercise provider transport, live SSH/Boat, physical offsite recovery, process relocation or hardware persistence.
- Selected content returns to the original existing linked checkout. Git history, branches and staged indexes are not reconstructed on a fresh target. The original staged index stays unchanged while working-tree content changes.
- Snapshots compare regular-file bytes in these fixtures, not empty directories, directory attributes, symlink metadata or concurrent external writers. Source/metadata quiescence remains a caller obligation.
- Explicit rollback may overwrite selected edits made after apply; the test deliberately demonstrates that authorized behavior. Exact apply retry alone preserves later edits in the tested flow.
- Conflict originals and refusal are covered; successful manual resolution and subsequent application are not newly qualified by these cases.
- No new ownership registry, scheduler, coordinator, deployment, automatic failover or billable resource is introduced.
