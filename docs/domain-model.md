# Domain model

The main distinction is **identity versus description**, not whether a Rust variable uses `mut`.

An entity represents the same thing as its state changes. A value object describes a fact and is compared or replaced by its contents. A command asks for work; a result or snapshot records what was observed. Public data fields can be mutable Rust values without becoming domain entities. Validated IDs and paths keep their inner representation private so callers cannot bypass their invariants.

## Context boundaries

| Context | Owns | Communicates with other contexts through |
| --- | --- | --- |
| Workspace management | Target identity, provider selection, ownership, file/port access, and resource lifecycle. | `WorkspaceRef`, `BackendDescriptor`, typed file requests, `IProvider`, and `IBackend`. |
| Execution | Process launch identity, lifecycle observations, output, cancellation, and deadlines. | `RunRef`, `CommandSpec`, `ExecutionRequest`, `RunState`, and output cursors. |
| Synchronization | The local/remote baseline, staged reviews, publication recovery, and handoff progress. | `SyncSession`, `HandoffReceipt`, checkpoint references, and `IWorkflowRepository`. |
| Checkpoint storage | Immutable selected-file artifacts, archive validation, three-way reconciliation, and recovery snapshots. | `ICheckpointEngine` and checkpoint/reconciliation/recovery value types. |
| Runtime preparation | Workload requirements, installation/setup recipes, credential needs, and native authentication progress. | `IRuntimeHost`, `ICredentialResolver`, `IPreparationStore`, and prepared-runtime values. |

These are ownership boundaries inside one Rust library, not separate network services. Provider adapters and the native supervisor are infrastructure. The supervisor protocol is a transport contract; it is not the domain model or a second agent platform.

## Workspace and execution concepts

| Concept | DDD role and meaning | What changes |
| --- | --- | --- |
| `Workspace` | Identity-bearing application handle to a target resource. Clones refer to the same workspace. | The provider-owned resource state changes; the handle does not store a second authoritative copy. |
| `WorkspaceId` | Validated value naming the workspace in the application. | A different ID means a different identity. |
| `ProviderId` | Validated value selecting an injected adapter registration. | Replacing it selects a different provider registration. |
| `WorkspaceRef` | Reconnect value containing workspace ID, provider ID, and provider-owned locator metadata. | Persist and reuse it; it carries no live connection or credentials. |
| `BackendDescriptor` | Snapshot of roots, isolation, ownership, and supported operations. | A snapshot describes the connection; poll lifecycle separately. |
| `WorkspaceState` | Lifecycle observation such as ready or stopped. | Observations change as the resource progresses. |
| `OpenRequest` | Command to create or attach a workspace under a stable identity. | Retrying the same identity must retain the same request intent. |
| `ProviderOptions` / `WorkspaceLocator` | Explicit schema-erasure values at the extensible provider boundary. | The owning adapter encodes/decodes a named struct; other contexts do not inspect an arbitrary map. |
| `WorkspacePath` | Validated relative-path value. | Construct another value to choose another path. Lexical validation does not replace backend symlink checks. |
| `Run` | Identity-bearing application handle to one process. | Process state changes while its ID stays stable. Dropping a handle does not stop it. |
| `RunId` / `RunRef` | Local run identity and workspace-qualified reconnect identity. They share `execution/run.rs` because they describe the same concept at different scopes. | Neither starts a process by itself. |
| `RunState` | Observation of starting, running, terminal, or unknown process state. | `Lost` is uncertainty, not confirmation that the process stopped. |
| `CommandSpec` / `LaunchRequest` | Process configuration and an identified managed-launch command. | A new intent requires a new run identity; retries cannot silently change the command. |
| `ExecutionRequest` | Execution-boundary value combining a command with transient secrets/private stdin. | Resolve credentials at delivery time; do not persist the request. |
| `OutputCursor` / `OutputPage` | Continuation value and bounded output result. | Cursors advance; retention can create an explicit gap. |

`Capabilities`, `Isolation`, `Ownership`, file chunks, resource observations, port requests, and deletion states are descriptive values. A `DeleteOperation` is a persisted progress receipt; its operation identity remains stable while provider-reported progress changes.

## Synchronization and runtime concepts

A `SyncSession` is a persisted workflow entity: its operation ID stays fixed while initialization, baseline, review, and publication progress change. A `HandoffReceipt` is the corresponding entity snapshot for one capture/restore/launch workflow. `SyncService` owns transitions under an exclusive lease. `WorkflowRecord` names the workflow kind explicitly so a repository does not accept an unexplained JSON object.

A `Checkpoint` represents an immutable captured artifact. Its reference identifies that artifact; it does not contain a running process. A `Reconciliation` is a staged review snapshot with a local destination fingerprint and preserved conflicts. Apply and recovery receipts describe publication progress. Resolving a conflict changes the review output; it does not mutate the original checkpoint.

A runtime recipe describes what should be available and how explicitly authorized setup can establish it. Resource observations describe what is available now. A preparation receipt records progress under a stable request identity. `PreparedRuntime` is the resulting launch capability. Authentication states and sessions describe native login progress; `CredentialRef` identifies an application-owned secret, while `SecretValue` carries the transient plaintext only at the credential boundary.

## Interfaces and implementations

Interfaces use the requested `I` prefix. Their names describe the role required by a use case, not the storage technology implementing it.

| Interface | Concrete implementation examples |
| --- | --- |
| `IWorkspaceRepository` | `FileStateRepository`, `MemoryStateRepository` |
| `IWorkflowRepository` | The same repositories, with typed synchronization records |
| `IWorkflowLease` | File-lock and memory-lock lifetime guards |
| `IProvider` | `LocalProvider`, `BoatProvider`, `SshProvider`, `ContainerProvider` |
| `IBackend` | The connected provider backend |
| `ICheckpointEngine` | `GitCheckpointEngine` |
| `IRuntimeHost` | `BackendRuntimeHost` |
| `ICredentialResolver` | The application’s credential store adapter, or `NoCredentials` |
| `IPreparationStore` | `FilePreparationStore`, `MemoryPreparationStore` |
| `IPreparationLease` | Per-preparation memory or file-lock operation ownership |
| `ICommandRunner` | `SystemCommandRunner`, or a recording test implementation |

`FileStateRepository` therefore implements `IWorkspaceRepository` and `IWorkflowRepository`. Calling those interfaces `IFileStateRepository` would make the application depend on a file-specific implementation choice even when it injects a database or in-memory store.
