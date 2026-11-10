# Retained workspace fingerprint verification

Retained continuation admission verifies the successfully stopped predecessor
and the acknowledged canonical checkpoint, then compares the current selected
workspace tree with that checkpoint. The recovery lease, epoch/head checks,
caller assertion that all writers are stopped and prelaunch retry verification
remain required.

`SyncService::fingerprint_workspace` uses the same target export, bounded chunk
transfer, cursor/length checks and complete transfer checksum verification as
`capture_workspace`. The downloaded archive is passed to
`ICheckpointEngine::fingerprint_archive`. Target transfer staging cleanup is
attempted after acquisition succeeds or fails, before the engine consumes the
controller-owned bytes. Cleanup failure retains the existing best-effort policy;
it is not confirmation that no target staging remains. Publication still calls
capture and creates a durable checkpoint.

The built-in `GitCheckpointEngine` validates the whole archive with its existing
parser, including content digests, sizes, paths, selection rules, supported modes,
symlinks and configured bounds. It derives the fingerprint without creating a
checkpoint, packing a Git bundle, invoking Git or mutating engine state. An
invalid archive cannot supply a trusted fingerprint.

The new engine port preserves existing implementations: its compatibility default
calls `capture_archive` once with default capture options and returns that
checkpoint's fingerprint. That default can publish an artifact, just as prior
continuation verification did. Custom overrides own their documented behavior;
errors propagate directly without an automatic second attempt or fallback.
Thus no-artifact verification is a guarantee of the built-in Git implementation,
not every injected engine.

This removes discarded controller proof checkpoints from built-in retained
admission. It still exports and downloads the complete selected tree. Target
export and canonical checkpoint validation may still use Git. Buffering and
archive decoding remain bounded by existing limits and have not become streaming
or zero-copy. No bandwidth, peak-memory, cross-generation deduplication or COW
improvement is claimed.

The opportunity was identified while comparing the existing implementation with
the credited [LayerFS design study](research/layerfs-learning.md). No upstream
implementation was copied and no dependency was introduced. Native tests compare
artifact counts and observed timings, prove actual publication still creates a
checkpoint, and retain divergence, retry, transport-integrity and cleanup oracles.
Timings are functional observations, not a statistical speedup baseline.
