# Architecture

Workspace SDK is a Rust library with an explicit native Rust supervisor executable. Applications own cloud accounts, target paths, credential stores, UI, and agent logic. The SDK coordinates work through injected interfaces.

## Boundaries and dependency direction

| Layer / context | Source | Responsibility |
| --- | --- | --- |
| Workspace domain | `src/domain/workspace/` | Identity, descriptors, resources, paths, file/port commands, and deletion observations. |
| Execution domain | `src/domain/execution/` | Run identity, commands, transient input, state, and output values. |
| Synchronization domain | `src/domain/synchronization/` | Typed session and handoff progress records. |
| Application | `src/application/` | Open/reconnect, managed-run handles, sync, and handoff use cases. |
| Core interfaces | `src/ports/` | `IProvider`, `IBackend`, and persistence/lease contracts. |
| Checkpoint context | `src/checkpoint/{domain,ports,infrastructure}/` | Immutable file artifacts, reconciliation, apply recovery, and native tree transport. |
| Runtime preparation context | `src/runtime/` | Typed requirements, recipes, auth progress, services, and injected host/credential/store interfaces. |
| Provider infrastructure | `src/providers/` | Native HTTP, SSH, local process, and container-engine adapters. |
| Target supervisor | `src/supervisor/`, `src/bin/workspace-supervisor.rs` | Native process/file operations behind a typed versioned protocol. |
| Persistence infrastructure | `src/infrastructure/` | Atomic file and in-memory implementations of repository interfaces. |

The [domain model guide](domain-model.md) explains which objects carry identity, which are values, and which are observations. Related concepts share a file within a context; `RunId`, `RunRef`, and `RunState` belong together rather than becoming unrelated files in a flat folder.

Application services depend on interfaces and domain values. Concrete providers implement the interfaces. Provider-specific options and reconnect locators are encoded from named structs only at the adapter registry boundary. Known supervisor operations and workflow records remain typed structs/enums throughout the implementation; JSON is their transport or persistence encoding.

## Why there is a supervisor

A client connection cannot own a remote process’s entire lifetime. Managed work needs a target-side component to retain run identity, capture bounded output, record completion, enforce deadlines, and stop the process group after the caller disconnects.

`workspace-supervisor` is that component, implemented in Rust. It receives a typed request, validates the workspace ownership/location context, and returns a typed result. A managed run has a detached supervising process and durable receipts. Workload output is read through cursor operations rather than mixed into protocol responses.

The supervisor does not contain an agent loop or model-provider login implementation. Runtime recipes launch native tools; those tools own their supported authentication flows. The private control root is separate from the synchronized project tree.

There is no embedded Python script assembly. `SupervisorDeployment` explicitly selects an installed executable or a caller-supplied artifact tagged for the target platform. An uploaded artifact must match the target OS/architecture. Applications must also select a build compatible with the target’s C runtime. Building a library on macOS does not automatically produce a Linux supervisor.

## Configuration and state

Project roots and private application state locations come from configuration. Boat and SSH adapters do not assume a user name or home directory. Container roots are configured for the selected image. Child names used for run records and scoped transfers are internal format conventions beneath those caller-selected roots.

`WorkspaceRef` is a non-secret reconnect value. `RunRef` adds a run identity in that workspace. `SecretEnvironment` and `SecretInput` are transient execution values; their contents are not written to normal command receipts or debug output. The wire transport must still carry those bytes to the target securely. Programs can print their own credentials, so workload output is not automatically sanitized.

The file repository uses typed workflow records and atomic replacement under locks. A workflow lease stays held through publication and receipt updates, including blocking work that outlives cancellation of an async caller. Known older workflow records are decoded through explicit typed migration; corrupt state is rejected rather than reset.

## File synchronization

Checkpoint storage owns selected-file filtering, immutable bundles, merge reviews, and recovery snapshots. The workspace transport moves a validated archive in bounded chunks. Native target import supports an empty destination or a matching interrupted retry; it never silently repairs a destination edited by another writer.

Return sync compares a shared baseline, local changes, and the remote snapshot. It stages a review without changing the local project. Publication requires conflict acknowledgement, the expected local fingerprint, and an explicit quiescent-writer assertion. The assertion is an application contract, not a lock on arbitrary editors.

Handoff requires a confirmed-stopped source run, captures selected files, restores an empty target, and starts an explicit continuation under a stable run identity. Recorded phases make retry behavior inspectable. `Lost` is not a safe-to-duplicate terminal state. VM memory and automatic conversation-format conversion are outside this contract.

## Boat client integration

The vendored Boat client tracks upstream `main` at `4d68256bb0b6a889633abc567fb09214681874db`, without source patches. The adapter delegates HTTP transport, retries, response limits, diagnostic redaction, readiness polling, command defaults, and text-file encoding to that client. Workspace creation uses its typed `CreateOptions` with the existing stable idempotency key. Hosting explicitly requests `public: false` and verifies the returned protection status.

Boat's [SSH access](https://docs.ascii.dev/box/ssh-access) supports piped stdin. Its documented [HTTP command request](https://docs.boat.dev/api/v1) and this client revision expose command text, working directory, timeout, and detached mode, but no separate stdin field. This HTTP adapter therefore retains ownership-checked private request files. Client-side debug redaction does not establish server-side logging guarantees.

The client's handoff journal records callback progress; callers still implement capture, copy, verification, and continuation. Workspace SDK retains its provider-independent workflow because it also owns filtered checkpoints, merge/apply recovery, source-run quiescence, and destination run identity. Boat prompt/event streams describe Boat agent activity, while this SDK supervises arbitrary workload processes. Substituting those APIs would change the execution contract.

## Current delivery

The active product is the Rust library plus Rust supervisor. The TypeScript package, its JSONL client CLI, and obsolete spike harnesses have been removed. Historical evidence is retained with its original scope; it is not proof of the rewritten supervisor. Registry publication and additional language bindings remain separate later work.

## Managed local editing

`application/edit` stages immutable candidate archives under session-qualified
revisions and delegates all validation, merge, resolution and publication to
`ICheckpointEngine`. Its effective limits are exposed through that port and
checked before session capture. `IEditCoordinator` isolates the local root/lease
seam; `LocalEditCoordinator` reuses the existing workflow lease repository with
a canonical-root key and directory-identity check. It serializes participating
SDK controllers only. Actual unmanaged-writer quiescence is caller-owned.

The checkpoint owner preflights uncaptured destination collisions before apply
intent/mutation, protecting ignored files from rollback removal. Live observations
use descriptor-relative POSIX directory enumeration on Linux/macOS. Native
exclusive creation still requires Linux anonymous-file support. UI, polling and
writer pause/resume remain host responsibilities. See [managed editing](state/managed-editing.md).
