# Checkpoint durability: independent quality review

Decision: **FINAL PASS** for
`15e9814c59eb53602e04c8c43ac633c15dfe6021`.

Independent reviewer: `/root/boat_cleanup_quality`. The reviewer did not implement
this change. It reviewed the actual constructor, filesystem publication helpers,
snapshot/import/export paths, native integration tests, guide and fixture fix
against Clean Code, SOLID, dependency direction, cohesion and bounded ownership.

## Change and findings

Publication uses a concrete private syscall seam. It syncs owned files and
directories before rename, then canonical parent ancestry. Errors propagate;
post-rename failure retains the uncertain artifact. Import retries and export
readiness revalidate stored bundle and cached tree and finish syncing. Constructor
retry does not mistake existing but previously unsynced directories for success.
No new public storage port, journal, schema or synchronization framework is added.

| Finding | Severity | Resolution and re-review |
| --- | --- | --- |
| Corruption tests initially removed `repo/objects`, but storage is a non-bare Git repository. | P2 | Both paths corrected to `repo/.git/objects` before the gate. Re-review checked `init_repo`; final executed tests reach and reject actual cached-object loss. Closed. |
| The first gate exposed canceled incomplete requests panicking the existing Boat fake server and an overly tight test deadline. | Runtime failure | Ignore incomplete input before recording/handler effects; add empty/partial-header/partial-body regression. Increase only test budgets, keeping completion, identity and request-count assertions. Separate delta review passed. |

No unresolved P1/P2 or lesser actionable finding remains in the accepted scope.

## Actual validation

[Run 39069885745](https://github.com/nessalabs/workspace-sdk/actions/runs/39069885745)
ran `scripts/quality-gate.sh` successfully on Ubuntu 24.04 and macOS 15. The
reviewer verified both checkout SHAs, successful script completion and individual
test records. Each OS executes all eleven new durability unit tests and three
native independent-store tests under both feature selections. All ten Boat mock
tests pass under all features; no-default runs zero Boat tests by configuration.
Both live Boat entrypoints remain ignored.

The full script includes formatting, warning-denying Clippy and docs, both
feature selections' builds/tests, native supervisor and managed/PTY performance
qualification. Each platform produced three managed and fifteen PTY debug-build
samples with bounded retained output. These samples establish qualification,
not a statistical regression baseline or checkpoint throughput improvement.
Full metadata, actual samples and raw log hashes are in
[qualification.json](../evidence/checkpoint-durability-2026-10-17/qualification.json).

Run 39069583255 passed eleven new unit tests on Ubuntu before failing in the old
Boat fixture; its macOS job was cancelled. It remains failed/cancelled evidence.
Tests were written before implementation seams, but no old-source red run or new
production mutation probes are claimed.

## Limits and costs

Readable, syncable canonical ancestry through the filesystem root is required.
Filesystem/hardware persistence semantics and actual power loss are not proven.
Separate local directories and controller reopening establish dependency isolation,
not physical off-host recovery or distributed authority. Export now validates
cached Git content and syncs artifacts under the engine lock, adding work
proportional to content and entry count. No incremental optimization is claimed.
Live Boat behavior and production SDK-owned create/bootstrap durability are unchanged.
