# Implementation plan and scope

The active delivery is a Rust SDK and Rust target supervisor. Additional language bindings, a client CLI, and managed cloud accounts are deferred. Applications bring their own provider accounts, machine access, credentials, and explicit target paths.

## Current refactor

- Organize related concepts into workspace, execution, synchronization, checkpoint, and runtime-preparation contexts.
- Document entity identity, value semantics, lifecycle snapshots, commands, fields, and invariants in Rustdoc.
- Name injected interfaces with `I`, keeping concrete implementation choices out of interface names.
- Use typed supervisor requests/results and typed workflow repository records.
- Replace embedded Python with a compiled Rust supervisor and native Rust tree/runtime operations.
- Make workspace roots and remote supervisor deployment explicit configuration.
- Remove the TypeScript package, JSONL client CLI, obsolete spikes, and stale distribution bundles.

## Validation and release gates

Native process tests must cover reconnect after client exit, duplicate launch rejection, bounded output, deadlines, process-group cancellation, transient secret input, and unknown supervisor outcomes. File tests must cover binary chunks, symlink boundaries, mandatory exclusions, conflicts, stale publication inputs, and interrupted recovery. Provider tests must cover ownership and correctly selected supervisor artifacts.

A remote artifact must be built for and tested on the intended platform. Mocked SSH/container commands are not a live deployment. Native agent subscription authentication and inference require a real user-authorized login flow and are separate from synthetic process tests.

Box deletion completion remains a provider limitation in historical live checks: accepted deletion operations could remain blocked. Continue returning/persisting that state honestly; do not equate a missing resource lookup with completed cleanup.

## Later work

After the Rust API and native deployments are validated, prepare registry distribution and artifact release automation. Additional language bindings can then be designed against the documented Rust concepts. Automatic file watching, distributed controllers, VM-memory migration, and automatic agent-conversation format conversion remain outside the current contract.
