# Checkpoint artifact streaming: adversarial acceptance review

Reviewer: independent agent `/root/workspace_handoff_adversarial_review`.

**FINAL PASS** for source commit
`539d95c6dc1fea6f2e4a453ceb5541c12ae06bb9`. No unresolved actionable P1/P2
findings remain. The accepted implementation change is confined to
`src/checkpoint/infrastructure/filesystem.rs`: stream directory entries rather
than collecting and sorting each directory, plus three resource-bound tests.

## Findings and disposition

**Test coverage correction, closed before execution.** The initial entry/byte
fixtures used `max_files: 0`, which the public `with_limits` API rejects. That
proved private arithmetic on an unreachable public configuration. The final
fixtures use positive limits: 5,121 empty metadata entries exceed the 5,120-entry
allowance; two 2,052-byte files independently fit but their 4,104-byte total
exceeds the 4,102-byte allowance regardless of enumeration order. Source
re-review passed. This finding was identified by source inspection; no executed
red probe against the initial fixtures is claimed.

No production correction was required in this review. Sibling traversal order
and which error appears first among multiple invalid siblings may change. Neither
is a promised public guarantee.

## Correctness assessment

The streamed loop still charges the shared entry budget for each yielded item,
propagates iterator errors through `child?`, rejects symlinks and special files,
charges aggregate regular-file lengths with checked subtraction, and refuses
unsupported depth before opening that descendant. Metadata errors propagate.
There is no partial-success conversion or ignored traversal error.

Every accepted regular file is synced before its containing directory. A child
directory completes recursively before its parent directory is synced. The
publication helper performs rename only after the entire walk succeeds, then
syncs canonical parent ancestry. Removing sibling sorting does not change this
required partial order. A refusal may follow some harmless earlier syncs, but
cannot reach the rename or acknowledge publication.

The existing engine lock and SDK-owned artifact assumption remain unchanged.
Streaming is not a new filesystem snapshot or protection against an unrelated
writer replacing private paths concurrently. Existing no-follow checks and
publication validation are retained. The implementation removes the explicit
per-directory child vector and sorting; it does not promise constant total
memory or quantify a measured speedup. Recursion and directory iterators remain
bounded by the existing depth limit.

The new bound-test callback intentionally does not execute fsyncs. It observes
real fixture entries and metadata and checks the exact refusal, no rename
attempt, no destination and a retained stage. Its unrelated `previous` fixture
also remains byte-identical. That last assertion is a non-modification check,
not proof that a canonical recovery head was inspected. Actual syscall ordering,
pre/post-rename failure handling, snapshot-save integration and exact import
retry remain covered by the existing tests.

Post-rename sync failures still retain the visible uncertain artifact. Exact
import retry still validates and resyncs that identity; this refactor does not
add deletion, identity expiry, replay permission or a new persistence format.

## Executed acceptance evidence

I inspected the actual completed metadata and raw logs for
[run 39073806361](https://github.com/nessalabs/workspace-sdk/actions/runs/39073806361).
Both Ubuntu 24.04 and macOS 15 checked out the exact accepted SHA and completed
`scripts/quality-gate.sh` successfully. All three new bound tests passed under
both feature selections on each OS. I also verified both executions per OS of
the retained child-before-parent ordering, publication-boundary failure,
post-rename import retry and snapshot-save integration tests. The full script
includes formatting, strict all-target/all-feature Clippy, both feature test
configurations and example builds, documentation and performance qualification.

The [qualification evidence](../evidence/artifact-stream-2026-10-17/qualification.json)
contains actual job metadata, commands, samples and raw-log hash. I independently
matched its raw-log SHA-256
`db5483f04055c6252a71222b215c231a826f031aa22a51d4d2ba904cd958f332`
and both complete performance objects to the raw logs. Each platform recorded
three managed-process and fifteen PTY samples, with debug assertions: Linux
x86_64 and macOS aarch64. These qualify the existing process scenarios; they are
not an artifact-walk memory benchmark, statistical baseline or speedup proof.

## Limits

No physical power-loss experiment, new production mutation probe, remote-mount
persistence qualification or live provider test was performed for this increment.
Storage still must support the existing file/directory sync and atomic-rename
contract, including syncable canonical ancestry. The tests establish bounded
refusal and preserved publication ordering, not arbitrary concurrent private-store
mutation safety or aggregate artifact garbage collection. No build, source edit,
resource operation or extra test run was performed by this reviewer.
