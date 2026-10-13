# Using the Rust SDK

## Register infrastructure explicitly

Create a `WorkspaceSdk` with an injected `IWorkspaceRepository`, then register each `IProvider`. `FileStateRepository` is the default durable implementation and also implements `IWorkflowRepository`; `MemoryStateRepository` is useful for ephemeral consumers and tests. Keep state outside project roots.

The provider IDs default to `local`, `boat`, `ssh`, and `container`. Each adapter supports `with_id(ProviderId)` so one application can register multiple SSH hosts or cloud accounts. Credentials belong to configured providers and credential resolvers, never to `WorkspaceRef`. The pinned Boat client is reexported as `workspace_sdk::boat_client`, so consumers need no separate sibling dependency.

| IProvider | Construction | Open configuration |
| --- | --- | --- |
| Local | `LocalProvider::new(LocalConfig::trusted_host(base_dir))` | `LocalOpenOptions { attach }`; otherwise creates an owned project. |
| Boat | `BoatProvider::new(explicit_box_api)` | `BoatOpenOptions::new(root)`: explicit target root, optional existing Box ID, and new-Box type/TTL/environment. |
| SSH | `SshProvider::new(SshConfig::trusted_host(host, root))` | `SshOpenOptions { attach }`; existing host keys and SSH login must already work. |
| Container | `ContainerProvider::new(ContainerConfig::new(engine, image, root))` | `ContainerOpenOptions { container_id }`; otherwise creates a labeled container/volume. |

Build `OpenRequest::new(workspace_id, &options)` from the provider’s named options struct. `ProviderOptions` erases the concrete type only for registry dispatch; the adapter decodes it immediately into its own schema. Resource requirements are validated or rejected; they are never silently treated as provisioned. Boat sizing currently uses an explicit Box type. Runtime resource checks can verify the CPU, memory, disk, platform and GPU needs of an existing host.

Local and SSH execution share the user's host permissions. Container isolation depends on the selected engine/image; the SDK does not claim a hardware security boundary. Attachments disable destructive target lifecycle operations.

## Deploy the Rust supervisor

Build `workspace-supervisor` for the target platform. `LocalConfig.supervisor` names the local executable. For remote providers, configure `SupervisorDeployment::Preinstalled { executable }` or `SupervisorDeployment::Upload { artifact, executable }`. `SupervisorArtifact` contains the local binary path and `TargetPlatform`; the adapter checks the target OS/architecture before upload. The supplied build must also support the target’s C runtime.

All roots are explicit: local application directories, SSH and Boat target roots, and the container mount root. No adapter assumes `/home/user`. An upload destination is also an explicit absolute target path. Keep private application state and credentials outside synchronized project roots.

The helper is Rust, so targets using a prebuilt artifact need no Python interpreter or Rust compiler. They still need the tools required by the workload and Git for file-tree checkpoint policy. Remote transport bootstrap uses the provider’s native file API or ordinary SSH/container transfer tools; it does not embed an interpreted supervisor.

## Workspaces and runs

`open` reserves a workspace ID for one creation request. Repeating that request reconnects its target; changing the request under the same ID returns a conflict. `workspace(id)` looks up a saved descriptor, and `connect(reference)` reconnects an externally saved reference without recreating missing state.

Use `CommandSpec::new(program).args(...)`. A shell is used only if you explicitly choose `sh`, `bash`, or another shell as the program. Working directories and file API paths are workspace-relative and validated; arbitrary program execution remains as powerful as the chosen target's permissions.

`exec` waits for a bounded managed command and returns exit code, timeout and truncation flags. Use `start` for detached work. For retries after a lost response, call `start_with` using the same `RunId` and exactly the same request. Reusing an ID with different command, credentials or output retention settings is a conflict.

Persist `RunRef`; reconnect the workspace and call `workspace.run(reference.id)`. `Run::wait` only bounds the caller's wait. It does not cancel work. `cancel` requests process-group termination. A `Lost` state means the supervisor disappeared and execution outcome is unknown; investigate before starting a replacement.

Output cursors are absolute byte offsets. A page's `gap` signals that older bytes were evicted from the bounded tail. Logs contain UTF-8 text with replacement characters for invalid sequences; use the binary file API for binary artifacts. A process may print secrets into its output, so apply your application's output access/retention policy.

## Credentials and preparation

Inject `ICredentialResolver`, `IRuntimeHost`, and `IPreparationStore` into `PreparationService`. `BackendRuntimeHost::new(workspace.backend())` connects runtime preparation to any registered workspace backend.

A `CredentialRef` is an application-owned identifier. The resolver returns `CredentialResolution::Available(SecretValue)`, `Missing`, or `Expired`. Only declare variables a workload or setup step requires. Ordinary command/recipe configuration is serializable; put credentials in secret fields and keep credentials out of arguments.

Build a `PreparationRequest` with a stable ID, matching target ID, private `owned_root` under `workspace.descriptor().control_root`, workspace working directory, `RuntimeRecipe`, `EnvironmentSpec`, and explicit `Authentication` choice. Recipe constructors `runtime::codex(version)`, `opencode(version, provider)`, `claude_code(version)`, and `generic(id, program, version)` require a pinned version.

The request authorizes its declared installer and setup commands. Existing compatible tools are reused. Installations use an owned prefix rather than replacing global tools. `SetupStep` pairs a read-only probe with an idempotent apply command; use it for Python dependencies, model/data downloads, or a custom server. Artifact storage is application-owned, and artifact paths are ordinary files subject to checkpoint size/filter rules.

`prepare` returns one of:

- `Ready`: build a command with `PreparedRuntime::launch_command`, then use `BackendRuntimeHost::launch`.
- `NeedsInput`: ask the user for the named missing or expired credential, update the resolver, and retry the same request.
- `NeedsAuthentication`: show the native authentication action and call `start_authentication` when the user chooses it. Poll its managed process/output and then check authentication again.
- `SetupFailed`: report the failed stage and retryability; repair the environment or update the recipe under a new request identity.

Codex and OpenCode handle their own native ChatGPT authentication. Native Claude Code keeps its supported login choices; an interactive terminal requirement is returned explicitly. Claude through OpenCode or a custom Agent SDK uses API or supported third-party credentials. Choosing subscription authentication never silently switches to API billing. See [authentication sources](research.md) for current product constraints.

`IRuntimeHost::provision_private_file` is available for an application's explicit private-file integration. The SDK only includes a built-in auth-cache provisioning action for Codex's documented file-backed cache. Ordinary sync never copies agent auth directories.

For an OpenCode server, `PreparedRuntime::opencode_server(port, password)` requires a password and defaults to loopback. Exposing a service requires both an intentional bind address and the target's supported private/authenticated port mechanism.

## Checkpoints and conflict review

Inject `GitCheckpointEngine::new(git_executable, state_directory)` into `SyncService` with a `IWorkflowRepository`. The default limits are 25,000 files, 32 MiB per file and 256 MiB of selected content. Large training datasets/model weights should be separate artifacts or use explicit, reviewed limits.

`SyncService::initialize(id, local_path, workspace, true)` captures the local baseline and populates an empty target. The final flag asserts that **all writers are stopped**. It is a contract with your application, not a filesystem lock on an editor. Retrying initialization retains the original baseline.

After a workload stops, call `sync_back`. It returns a `Reconciliation` containing a review directory, preserved original versions, conflicts, and the expected destination fingerprint. This operation leaves the local project unchanged.

For conflicts, edit the review directory and call `ICheckpointEngine::accept_resolution(reference, resolved_paths)`, acknowledging every conflicted path. Then call `apply_sync` with `ApplyOptions { quiescent: true, expected_fingerprint }`. Clean merges also require explicit apply. If the destination changed since staging, stage a new review instead of overwriting the edits.

Apply records a recovery checkpoint before changing files. Keep the returned recovery reference. `ICheckpointEngine::rollback` requires exclusive access and restores the selected pre-apply files; it can overwrite later selected-file edits, so review them first. An interrupted apply is recovered under the same reconciliation identity. Conflicts, incomplete restores and unknown run outcomes remain visible errors.

Mandatory exclusions include Git metadata, credentials, agent auth state, `.env*`, private keys, common dependency/build caches and SDK control state. `.gitignore` handles untracked files and `.workspaceignore` adds project-specific exclusions. Local captures report excluded paths. Binary content, executable bits and safe relative symlinks are supported; submodules, unresolved LFS pointers, special files and escaping symlinks fail explicitly.

`export`/`import` move immutable Git checkpoint bundles between controllers. `export_checkpoint_archive`/`capture_archive` preserve an already-selected file tree over provider transfers, including tracked files that match `.gitignore`.

## Handoff

Ask the source workload to flush portable state and exit. Confirm its managed run is terminal. Open an empty destination, then call:

```rust,ignore
let receipt = sync.handoff(
    operation_id,
    &source_workspace,
    &source_run,
    &destination_workspace,
    continuation_command.into(),
    true, // all other source/destination writers are stopped
).await?;
```

The service captures files, restores the target, and launches the continuation under a deterministic run ID. Persisted phases make retries resume the same operation. Failures keep the source stopped. `Lost` source state prevents duplication.

This is portable file state plus an explicit continuation command. It does not migrate VM memory or automatically translate Codex, Claude or OpenCode conversation stores. The runnable [local workflow example](../examples/local_workflow.rs) demonstrates the shared sync path; the opt-in [Box integration test](../tests/boat_live.rs) exercises Box-to-Box handoff with a deterministic worker.

## Deletion and extension

`destroy` is restricted to owned resources. Save its `DeleteOperation` and inspect asynchronous provider cleanup until completed. Box may report blocked cleanup even after a delete request is accepted; a missing Box lookup does not prove deletion finished. Local deletion receipts persist outside the deleted workspace. SSH target deletion is currently unsupported; stopping its managed runs is supported.

To add another cloud, implement `IProvider` and `IBackend`, including truthful capability and ownership descriptors. You may use native HTTP APIs, SSH or your own process transport. No common remote server is required. Register the adapter through the builder and run the conformance scenarios used by the supplied providers.
