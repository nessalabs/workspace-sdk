# Native tree import retry durability: adversarial acceptance review

Reviewer: independent agent `/root/workspace_handoff_adversarial_review`.

**FINAL PASS** for source commit
`e6fbc1a8dbccb62aeb5f4efd136eae2b69a82877`. No unresolved actionable P1/P2
findings remain. Scope: native import receipt publication and matching-retry
syncing, the shared private syscall seam, ancestor-helper visibility, documentation
and nine regression tests. No persistence format or retry-identity policy changes.

## Findings, corrections and re-review

1. **P2 — matching Pending files bypassed unfinished directory durability.**
   A file could be hardlinked or symlinked before a parent sync failed. Retry
   accepted matching bytes and skipped publication, including that parent's
   sync, then acknowledged Complete. The final implementation reflushes validated
   selected regular files and parent chains before completion, including files
   that already exist. Source re-review and executed nested-file failure/retry
   tests close this finding.
2. **P2 — visible Complete was treated as durable acknowledgement.**
   Receipt persist can succeed before its metadata-directory sync fails. The old
   Complete path validated contents but returned without completing that boundary.
   It now syncs selected contents, the receipt, metadata directory, project root
   and canonical ancestry before returning. The regression fails the actual
   metadata-directory callback after the real persisted receipt reads Complete;
   both the first attempt and matching retry must propagate the injected error.
3. **P2 — Pending intent ancestry was not guaranteed before file effects.**
   A prior interrupted metadata-directory creation can leave its name visible
   without a successful project-root sync. Syncing only inside that directory
   does not finish the intent-before-effects boundary. `save_receipt` now performs
   atomic publication and canonical ancestor sync before permitting publication.
   The new repeated failure test requires Pending to be visible and the selected
   path absent at project-root sync, then requires refusal before file creation.
4. **P2 — draft prefix-memory expansion, closed before gate.** An intermediate helper
   collected every full parent-path prefix in a set. The final helper walks
   parents directly instead, avoiding that extra prefix collection and its
   memory expansion. Shared directories may be synced repeatedly. Existing
   archive/selection/path limits still apply; no total-process memory claim is made.
5. **Test ordering correction, closed before gate.** Adding the required early
   Pending ancestry sync made the first root sync occur before selected-directory
   reflush. The draft assertion incorrectly compared against that first root.
   It now requires a later root sync after the nested directory, while the
   separate intent test enforces the earlier boundary. This was a source-detected
   test issue, not a claimed executed red reproduction.

The original gaps were identified through actual code tracing. Existing tests
checked matching file contents but did not establish these sync/retry boundaries.
No production mutation probes or physical power-loss reproduction are claimed.

## Final source assessment

The atomic receipt helper writes a real temporary file, syncs it through the
existing concrete `PublicationStep` boundary, persists it, then syncs its parent.
The import-specific receipt operation additionally syncs canonical ancestry.
Any failure is returned; a visible uncertain receipt is retained for inspection
and retry rather than deleted or interpreted as permission to overwrite.

Validation remains ahead of retry syncing: archive checksum and selection,
current selected entries, direct paths and Complete-tree equality must match.
Changed partial data conflicts before any retry sync callback or rewrite.
Existing new-only hardlink/symlink publication remains intact. Complete retries
perform syncing without republishing files or rewriting the journal. A regular
file's inode and receipt bytes are checked unchanged in the relevant regression.

Symlinks are validated using link metadata and their stored target text. The
sync helper opens only selected regular files; symlink parents are synced without
opening the link or its target. Directories flush from child toward root, and the
root plus canonical ancestry are included even for an empty tree. Direct parent
iteration does not retain a collection of all prefixes. Extra repeated directory
syncs are a deliberate work/space tradeoff, not a throughput improvement claim.

Public import still owns the existing project lock, and its all-writers-stopped
precondition remains necessary. These path checks and fsyncs do not create a
race-proof filesystem sandbox against arbitrary external writers.

## Executed evidence and acceptance

I inspected actual metadata and raw logs for
[run 39076072324](https://github.com/nessalabs/workspace-sdk/actions/runs/39076072324).
Both Ubuntu 24.04 and macOS 15 checked out the exact reviewed SHA and completed
the full `scripts/quality-gate.sh` successfully. Each of the nine named new import
unit tests passed under both feature selections on each OS, including Pending
intent failure, repeated matching ancestor failure, Complete selected-file and
receipt failures, post-persist metadata failure, bottom-up ancestry, empty tree,
symlink handling and changed-partial refusal. These use real fixture files,
receipts and publication effects with injected syscall-boundary errors; they do
not simulate a physical disk losing power.

The gate also runs formatting, strict all-target/all-feature Clippy, both feature
suite and example configurations, documentation and process performance scenarios.
The [qualification record](../evidence/import-retry-durability-2026-10-17/qualification.json)
preserves their scope and evidence. I independently matched both complete
performance objects to the raw run log: three managed-process and fifteen PTY
samples per platform, with debug assertions, on Linux x86_64 and macOS aarch64.
Those samples qualify existing runtime scenarios, not import fsync latency,
statistical regressions or a measured speedup.
Qualification job metadata and its raw-log SHA-256 also match the inspected
operator evidence exactly:
`417cfff5a722ad891f7a789575df2f206ce209b23261cae2992ffb8cc24e37ba`.

## Remaining limits

Storage must support the existing atomic-rename and file/directory-sync contract,
including syncable canonical ancestry. Successful syscalls are not proof that a
remote mount, disk controller or physical host preserves data through power loss.
Export retains its existing immediate-parent atomic-write sync scope; this review
does not qualify complete remote export durability. No offsite replication,
automatic failover, independent external-writer fencing,
aggregate orphan cleanup or live provider qualification is added here. Caller
quiescence and separately durable authority/artifact storage remain application
responsibilities. The reviewer made no source edits, builds, resource calls or
additional test runs; this record is the only authored file for final review.
