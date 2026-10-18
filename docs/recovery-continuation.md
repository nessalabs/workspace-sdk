# Continue an acknowledged workspace

`RecoveryService::continue_current` starts a new cooperative step in the exact
workspace used by the preceding successful, published attempt. It retains the
`WorkspaceRef` and physical root, allocates the next epoch and a new deterministic
`RunRef`, and uses the existing recovery repository, effect lease, revision CAS,
phases and target-local launch claim. Ordinary `recover` still creates a fresh
workspace.

The caller supplies `RecoveryContinuationRequest` with a stable `request_id`,
`expected_previous_epoch`, the exact observed `expected_committed` head, transient
`continuation` inputs, and `quiescent: true`. The assertion means the caller has
exclusive access and has stopped every other writer, including editors, direct
runs and unmanaged processes. The SDK cannot establish that global exclusion.
Keep the original request fields and command unchanged across retries.
Maintain exclusion until the owned operation finishes, even if its waiting
caller is dropped; dropping a waiter does not stop the accepted operation.

Before reserving a successor, the service requires the immediately preceding
authoritative run to have exited zero and its output to have been acknowledged
in the canonical head. It reconnects that exact workspace and compares the
selected current tree with the validated canonical checkpoint. Checkpoint IDs
may differ for identical contents; comparison uses the tree fingerprint.
Divergence returns a conflict without restoring, merging, deleting or launching.
Excluded paths remain outside this comparison.

The reserved attempt records retained-workspace provenance and the nonsecret
command fingerprint. New epochs use new deterministic run IDs. The service saves
launch intent before requesting the native launch claim. A prelaunch retry checks
the retained predecessor and selected contents again. After `LaunchRequested` or
`Started`, retry never recaptures or reapplies the older baseline: the successor
may already have written new output. An exact accepted retry returns the same run,
including after its publication.

`publish` checks the newly authoritative run and advances the generation normally.
Starting another run directly and then publishing against the old recovery
attempt would bypass this ownership sequence and is not a supported substitute.
Fresh and retained requests cannot retry one another's attempts.

Running, unknown, unreachable, unsuccessful, canceled or timed-out predecessors
cannot authorize a new retained step. This operation has no externally-fenced
override. An uncertain attempt stays pinned to its recorded reference; changing
providers, selecting an older workspace or replacing divergent files requires a
separate explicit operation. Epoch checks govern canonical publication; they do
not fence arbitrary external effects.

Verification captures selected contents through the existing checkpoint service
and creates an additional immutable artifact. It uses the checkpoint and provider
resource bounds; it is not a storage quota or free-resource reservation. The first
call reuses its verification within the same held lease and caller-established
exclusive boundary. A resumed prelaunch call verifies again. Callers own artifact
retention and offsite durability.

This API preserves a workspace between completed steps. It does not transfer
process memory, watch health, establish physical host termination, or qualify
application-specific session migration to another host.
