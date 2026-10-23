# Workspace SDK readiness checkpoint

## Ownership and source

The isolated branch `codex/sdk-readiness` starts from GitHub main
`0b7bbc0a9be0cce0713df05be56f2a63d6c14c30`, verified with `git ls-remote`
on 2026-10-23. The original checkout is
`/Users/nessa/Documents/NessaLabs/workspace-sdk`; its uncommitted historical
review-file addition remains untouched. Supported thread retrieval identified
“Design unified local-cloud SDK” as the completed previous owner. PRs #1 and #2
were already merged; this task does not redo them.

## Findings and work in progress

- P2: a dangling `state.json` symlink was mistaken for an absent store.
  The regression failed before the fix; metadata inspection now distinguishes
  genuine absence from invalid storage. File-state tests cover links, directories,
  corruption, unknown versions, oversized state, and legacy migration.
- P2: caller cancellation could skip transfer cleanup. The owned workflow
  implementation and deterministic cancellation/lease tests are in progress.
- Transfer boundary hardening rejects invalid cursor, chunk length, declared
  size, and checksum observations before importing.

This is a checkpoint, not an acceptance verdict. Final source identities,
commands, results, independent reviewers and remaining limitations will be
recorded before publication.
