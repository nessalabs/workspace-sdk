# Existing linked-worktree return: independent quality review

Reviewer: independent quality agent `/root/boat_cleanup_quality`, applying
`AGENTS.md` and `coding_standards.md`. Exact reviewed source:
`022f8859be2d65fe741a4da7d2b2419ea1e1f2ef`.

**FINAL PASS for this test-only qualification. No unresolved P1/P2 finding
affects this scope.** The decision follows actual source and full gate evidence
review; the separate adversarial review is independently required. This commit
adds three tests and four focused helpers in `tests/checkpoint_engine.rs`, with
no production behavior or public API change.

## Design findings and source review

No actionable P1/P2 was found in the frozen tests. The initial design assessment
identified a selection trap: restoring into a plain directory and then using
ordinary capture could omit tracked-but-ignored files because the destination
has no Git index. The implemented tests avoid that shortcut. They export the
selected checkpoint archive, copy it into a disposable target transfer path,
call actual `import_remote_tree`, edit the target, then use receipt-aware
`export_remote_tree` and `capture_archive`. They explicitly check retained
tracked-but-ignored content and its returned manifest membership. This is
existing public behavior exercised through real Git and filesystem boundaries,
without an alternate test implementation or new framework.

The fixture deliberately stages one value and then writes a different working
value before taking the baseline. Base capture therefore selects working-copy
bytes, while byte-identical index assertions establish that staging is retained.
The main checkout snapshot includes its regular files and common `.git`
metadata, including objects, refs, logs and both indexes. The linked `.git`
pointer and excluded local bytes are separately compared. Baselines follow all
intentional staging, and no refreshing Git status/diff command runs afterward.
This avoids confusing legitimate Git cache refreshes with SDK writes.

The three cases meaningfully qualify distinct boundaries:

- Clean return changes a tracked-but-ignored file, deletes another tracked
  file, and adds a target file. A disjoint local addition and selected untracked
  content survive. Reconcile leaves the linked tree unchanged; explicit apply
  changes the intended working files while preserving Git metadata. Retrying
  after a later user edit returns the original receipt and preserves that edit.
  Explicit rollback restores the exact pre-apply regular-file snapshot and
  records RolledBack.
- Nonquiescent apply and stale selected-destination apply both refuse without
  changing source/Git bytes and remain NotStarted.
- Divergent edits retain exact base/local/remote conflict blobs, leave the
  original checkout unchanged, and refuse unresolved apply and empty resolution
  acknowledgement. Completion of conflict resolution is outside this new case.

Helpers remain cohesive fixture setup, transfer, and assertion operations. The
tests do not pretend that selected-file rollback reconstructs Git history or
index staging. Rollback explicitly authorized after a later user edit can
overwrite that edit; the separate retry assertion establishes preservation by
retry alone.

## Actual qualification evidence

I verified final HEAD and the test file against the exact reviewed commit. The
commit changes only the test file, adding 262 lines. I independently compared
`docs/evidence/linked-worktree-return-2026-10-17/qualification.json` with raw
`../workspace-cloud-run-39087185162.{json,log}`: checkout SHA, complete job
metadata, conclusions, raw-log SHA-256, named test counts and complete extracted
performance JSON objects match.

Full cloud run **39087185162** succeeded on **Ubuntu 24.04 and macOS 15**. Each
of the three new names passed twice per OS, under all-features and
no-default-features: **12 new executions**. Both logs reach the actual
`scripts/quality-gate.sh` completion message. The script includes formatting,
warning-denying Clippy, builds, both test selections, warning-denying docs, and
both ignored runtime performance qualifications. Existing linked capture and
failure-policy cases remain in the passing full suite. No old-source red run
or mutation test is claimed for this qualification-only increment.

Each OS raw log contains the complete **3 managed and 15 PTY samples** recorded
in qualification evidence. Managed cancellation observations were
52.28–52.42 ms on Ubuntu and 102.87–192.26 ms on macOS; PTY cancellation was
52.27–64.23 ms and 62.32–254.78 ms respectively. These hosted debug samples
qualify existing runtime scenarios; they are neither a linked-return benchmark
nor a statistical regression baseline.

## Limits

The flow uses local disposable Git repositories, actual target archive helpers,
and file copying. It establishes no provider transport, live Boat/SSH, process
switching, fresh-host history/index migration, offsite recovery, or hardware
persistence guarantee. Snapshot equality covers regular-file paths and bytes in
these fixtures, not directory attributes, empty-directory identity, symlink
metadata, or concurrent adversaries. Callers supply the quiescence required for
apply and rollback. No local builds/tests, source edits, or commits were
performed by this reviewer.
