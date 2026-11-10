# Learning credited to LayerFS

Upstream: [Ephemeral-AI-Lab/LayerFS](https://github.com/Ephemeral-AI-Lab/layerfs),
reviewed at [`16e622e27278396c2cb822b0b0cc75ad03a7102c`](https://github.com/Ephemeral-AI-Lab/layerfs/tree/16e622e27278396c2cb822b0b0cc75ad03a7102c)
on 2026-10-18. Credit belongs to the upstream authors, whose
[MIT license](https://github.com/Ephemeral-AI-Lab/layerfs/blob/16e622e27278396c2cb822b0b0cc75ad03a7102c/LICENSE)
identifies copyright 2026 Ephemeral-AI. This is a design study: no upstream code,
assets or documentation passages were copied into the SDK, and no LayerFS
package or runtime was installed. Any later implementation reuse needs its own
license/notice review; this record is not a license grant for unrelated material.

The useful storage idea is to expose a complete logical workspace while sharing
unchanged physical content. The upstream
[overview](https://github.com/Ephemeral-AI-Lab/layerfs/blob/16e622e27278396c2cb822b0b0cc75ad03a7102c/README.md)
combines content addressing, chunking and copy-on-write to make forks and retained
states cheaper. This suggests measuring changed content and transfer/storage
cost separately from the size of the tree exposed to an agent.

The [SDK description](https://github.com/Ephemeral-AI-Lab/layerfs/blob/16e622e27278396c2cb822b0b0cc75ad03a7102c/sdk.md)
and actual [commit API](https://github.com/Ephemeral-AI-Lab/layerfs/blob/16e622e27278396c2cb822b0b0cc75ad03a7102c/core/crates/layerfs-api/src/sandbox/commit.rs)
keep command execution, filesystem publication and runtime teardown as separate
operations. Its [close API](https://github.com/Ephemeral-AI-Lab/layerfs/blob/16e622e27278396c2cb822b0b0cc75ad03a7102c/core/crates/layerfs-api/src/sandbox/close.rs)
retains failure ownership rather than inferring cleanup from an uncertain reply.
These are useful reference boundaries; they do not qualify this SDK's behavior.

## Comparison with this SDK

The following behavior predates this study and is not claimed as derived from
LayerFS: immutable selected-tree checkpoints, separate execution and checkpoint
publication, exact epoch/head admission, explicit quiescence, retained uncertain
owners, and narrow `ICheckpointEngine`, `IBackend` and `IRecoveryRepository` ports.

There is a specific storage gap: `save_tree_with` in
`src/checkpoint/infrastructure/snapshot.rs` creates a fresh Git repository and
standalone bundle for each capture. Git content identity inside one capture and
manifest parent metadata do not establish shared copy-on-write storage or delta
transfer across captures.

One concrete candidate inspired by this review is proof-only tree inspection.
`verify_retained` in `src/application/recovery_service/continuation.rs` calls
`capture_workspace` merely to compare a selected-tree fingerprint. That path
currently publishes another durable checkpoint whose identity the caller does
not use. A read-only inspection path could avoid the extra Git packing and
artifact publication while retaining complete archive checksum verification,
selection rules, bounds, exact predecessor checks, writer quiescence, the held
recovery lease and retry verification. The resulting [fingerprint inspection change](../retained-fingerprint-inspection.md)
implements this candidate using the existing validated archive parser. Its native
checks record artifact count/bytes, Git subprocess counts and observed timings;
peak-memory and bandwidth improvements are not established. Full archive download
remains unchanged. This is not a delta-transfer or COW claim.

The clean-child-environment increment was already designed and implemented before
this repository was shared. It is not attributed to LayerFS. This learning is
recorded for subsequent storage/ownership design work.

## Qualification limits

The upstream overview separates the released 0.1.6 preview from its replacement
`core` implementation. Its preview
[storage contract](https://github.com/Ephemeral-AI-Lab/layerfs/blob/16e622e27278396c2cb822b0b0cc75ad03a7102c/docs/versioned/0.1.6/storage-format.md)
does not promise crash or power-loss durability. The current
[core guide](https://github.com/Ephemeral-AI-Lab/layerfs/blob/16e622e27278396c2cb822b0b0cc75ad03a7102c/core/README.md)
describes a disposable WAL/OFF profile and remaining integrated qualification
limits. These source-specific limits prevent treating the study as evidence for
acknowledged off-machine recovery, live provider switching, or physical failure
safety. The existing SDK durability and execution-ownership requirements remain.
