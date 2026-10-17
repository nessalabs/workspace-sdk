# Using the Rust SDK

## Register infrastructure explicitly

Create a `WorkspaceSdk` with an injected `IWorkspaceRepository`, then register each `IProvider`. `FileStateRepository` is the default durable implementation and also implements `IWorkflowRepository`; `MemoryStateRepository` is useful for ephemeral consumers and tests. Keep state outside project roots.

The provider IDs default to `local`, `boat`, `ssh`, and `container`. Each adapter supports `with_id(ProviderId)` so one application can register multiple SSH hosts or cloud accounts. Credentials belong to configured providers and credential resolvers, never to `WorkspaceRef`. The pinned Boat client is reexported as `workspace_sdk::boat_client`, so consumers need no separate sibling dependency.

| IProvider | Construction | Open configuration |
| --- | --- | --- |
| Local | `LocalProvider::new(LocalConfig::trusted_host(base_dir))` | `LocalOpenOptions { attach }`; otherwise creates an owned project. |
| Boat | `BoatProvider::new(explicit_boat_api)` | `BoatOpenOptions::new(root)`: explicit target root, optional existing Boat ID, and new-Boat type/TTL/environment. |
| SSH | `SshProvider::new(SshConfig::trusted_host(host, root))` | `SshOpenOptions { attach }`; existing host keys and SSH login must already work. |
| Container | `ContainerProvider::new(ContainerConfig::new(engine, image, root))` | `ContainerOpenOptions { container_id }`; otherwise creates a labeled container/volume. |

Build `OpenRequest::new(workspace_id, &options)` from the provider’s named options struct. `ProviderOptions` erases the concrete type only for registry dispatch; the adapter decodes it immediately into its own schema. Resource requirements are validated or rejected; they are never silently treated as provisioned. Boat sizing currently uses an explicit Boat type. Runtime resource checks can verify the CPU, memory, disk, platform and GPU needs of an existing host.

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

Checkpoint capture/import acknowledge publication only after syncing the owned
artifact files, directories and final directory entry. Export also validates and
syncs existing artifacts before acknowledging readiness. Directory sync errors
are returned; a post-rename error can leave a visible checkpoint whose publication
is uncertain. Retry the same import or inspect/export that identity to validate
and sync it. Do not delete uncertain artifacts merely to permit a retry.

The storage filesystem must support file/directory sync and atomic rename.
Constructor setup syncs canonical directory ancestry through the filesystem root,
including when retrying previously interrupted setup. This is not proof that a
remote mount or disk controller honors persistence. Store both artifacts and
recovery authority independently of a host you need to survive. Full validation,
Git tree reads and artifact syncing add work proportional to checkpoint size and
entry count; this is a durability change, not an incremental sync optimization.

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

This is portable file state plus an explicit continuation command. It does not migrate VM memory or automatically translate Codex, Claude or OpenCode conversation stores. The runnable [local workflow example](../examples/local_workflow.rs) demonstrates the shared sync path; the opt-in [Boat integration test](../tests/boat_live.rs) exercises Boat-to-Boat handoff with a deterministic worker.

The ignored Boat test creates two billable fixtures only when explicitly selected.
It records creation intent, a stable API idempotency key and the returned provider
ID in a durable ledger before native SDK bootstrap. The fixture is created through
the raw Boat API, then connected through the SDK attachment path. This keeps the
cleanup handle available if readiness, supervisor upload or initialization fails;
it qualifies attachment, files and native runs rather than the SDK-owned creation
path. It does not fix the production provider's creation/bootstrap persistence gap.

Keep the ledger beside `BOAT_SDK_REPORT` until cleanup is confirmed. A failed or
uncertain creation retains its original identity; do not start another fixture to
hide the uncertainty. These tests do not establish the server's idempotency-key
retention period, so an old uncertain creation requires provider reconciliation
rather than an assumption that replay remains safe forever. A nonblocking
per-ledger lifetime lock prevents the live test and cleanup-only process from
overwriting each other's ownership records. Repeated error classifications are
deduplicated so retries do not grow the ledger indefinitely. The cleanup-only ignored test
`cleanup_registered_boat_fixtures` loads `BOAT_FIXTURE_LEDGER` and retries cleanup
without provisioning, uploading artifacts or launching workloads. Credentials may
be supplied through the hidden PTY prompt; keep them out of source, report files
and command arguments. Confirm the deletion operation belongs to the recorded
target and reaches `completed`. A blocked or pending operation, an accepted stop,
or a missing resource lookup is not confirmation. Incomplete cleanup fails the
live test even if all functional checks passed. Review unresolved cleanup with
the account owner before running another billable fixture.

Select one ignored entrypoint explicitly, rather than running every ignored Boat
test together:

```sh
cargo test --locked --all-features --test boat_live native_rust_box_workflow -- --ignored --exact --nocapture
cargo test --locked --all-features --test boat_live cleanup_registered_boat_fixtures -- --ignored --exact --nocapture
```

## Explicit recovery and placement

`RecoveryService` adds a durable placement decision around the existing workspace,
checkpoint transfer, and native process APIs. Configure it with an `Arc<WorkspaceSdk>`,
an `Arc<SyncService>`, and an `Arc<dyn IRecoveryRepository>`. The file and memory
repositories implement this port. Keep both the authority repository and immutable
checkpoint artifacts outside projects and on storage that survives the host you
intend to recover from. A directory on the failed laptop is not an offsite backup.

1. Capture a complete checkpoint at a consistent boundary. Call `create` with its
   reference and priority-ordered `RecoveryCandidate` values, for example laptop,
   development machine, then Boat. Each provider must be registered separately.
   The candidate configuration must create a fresh target, not attach a live project.
2. Call `recover` with a stable `RecoveryRequest` ID, `expected_previous_epoch: 0`,
   caller-observed available providers, a continuation command, and a safety mode.
   The first available candidate in the stored priority order wins. Its epoch,
   target, checkpoint and deterministic workspace/run IDs are saved before opening
   a target. An open failure remains pinned: retry it rather than silently moving
   possibly accepted work elsewhere.
3. Keep that request ID and expected epoch when retrying. Progress is
   `Preparing → Restored → LaunchRequested → Started`. Once launch intent is
   durable, changed nonsecret commands conflict. The native salted claim checks
   transient credentials on launch retries; a completed `Started` retry simply
   returns its historical reference and does not authenticate new credentials.
   Retry never restores over files
   changed by an already accepted run. `Started` means the native claim was accepted,
   not that the workload completed successfully.
4. After the run is terminal and **all** writers are stopped, `publish(id, epoch,
   true)` captures a new complete checkpoint and conditionally advances the head.
   Failed capture or stale epoch never advances it. This quiescence assertion does
   not stop editors or unmanaged writers for you.
   Publication is a new capture on every call. After an ambiguous publication
   response, inspect the committed generation and checkpoint before retrying;
   another call can create another complete generation and retained artifact.
5. To move again, submit a new request against the current epoch. Every move gets a
   fresh isolated workspace, even when returning to the laptop. Old directories
   remain available for inspection or reviewed reconciliation. Their identities are
   not recycled. Updating availability on an existing request does not preempt it.

`RecoverySafety::ConfirmedStopped` queries the previous run and refuses running,
lost, or unreachable work. `ExternallyFenced` is an explicit application assertion:
the caller guarantees that the old process cannot perform unfenced external
effects. The SDK cannot verify that promise. Epoch checks protect canonical
checkpoint publication; they do not stop an old process or prevent it from sending
an API request. A network timeout alone is not evidence that it stopped. An
unfinished attempt cannot be replaced by a new epoch, even with this assertion;
resolve its pinned launch uncertainty first.

The repository uses a nonexpiring, fail-fast session lease across provider work,
plus short atomic revision checks for authority writes. Concurrent operations on
that session return a conflict; they do not steal a live controller's lease.
All controllers must share the same authority implementation and lock semantics.
Once polled, create/recover/publish continue in an owned Tokio task if their caller
is dropped or times out. The task keeps its session lease through effects and
transfer cleanup. Read session state and retry the same identity to reconcile;
dropping a caller is not a stop command. Providers must bound their own operations.
Runtime or process loss can still interrupt work and leave transfer staging or
unpublished artifacts; there is no aggregate garbage collector for crash orphans.
This is explicit placement, not an automatic health monitor or a distributed
expiring ownership service. File repository format 3 reads format 1/2 records;
older binaries cannot read format 3. Back up authority state before upgrading and
do not run old binaries against the upgraded repository.

Checkpoint transfer still uses bounded full archives, with time and memory
proportional to selected project data. There is no incremental transfer or
deduplication claim. Periodic capture, remote artifact replication, health policy,
native agent conversation conversion, and abandoned cloud resource cleanup remain
application responsibilities. Poll deletion receipts to completion when cleaning up
billable targets; an accepted delete request is not proof billing stopped.

## Deletion and extension

`destroy` is restricted to owned resources. Save its `DeleteOperation` and inspect asynchronous provider cleanup until completed. Boat may report blocked cleanup even after a delete request is accepted; a missing Boat lookup does not prove deletion finished. Local deletion receipts persist outside the deleted workspace. SSH target deletion is currently unsupported; stopping its managed runs is supported.

To add another cloud, implement `IProvider` and `IBackend`, including truthful capability and ownership descriptors. You may use native HTTP APIs, SSH or your own process transport. No common remote server is required. Register the adapter through the builder and run the conformance scenarios used by the supplied providers.
