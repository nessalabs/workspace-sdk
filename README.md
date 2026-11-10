# Workspace SDK

A Rust SDK for running workloads on user-owned compute, reconnecting to managed processes, and moving selected project files between machines with conflict review.

The SDK is organized around workspace management, execution, synchronization, and runtime preparation. Your application supplies provider configuration, storage locations, credentials, and its agent loop. The target-side supervisor is also Rust. There is no Python supervisor, TypeScript package, or client JSONL CLI.

## Use the Rust library

Build the native supervisor first:

```sh
cargo build --locked --no-default-features --features portability --bin workspace-supervisor
cargo run --locked --no-default-features --features portability --example local_workflow -- target/debug/workspace-supervisor
```

The example owns temporary directories, runs a small workload, reviews/applies file changes, and removes its workspace. Local execution deliberately uses trusted host permissions. It does not create a sandbox.

Add the crate to your application:

```toml
[dependencies]
workspace_sdk = { path = "/path/to/workspace-sdk" }
```

```rust,no_run
use std::{path::PathBuf, sync::Arc};
use workspace_sdk::{
    CommandSpec, CreateRequest, ProviderId, Result, WorkspaceId, WorkspaceSdk,
    infrastructure::FileStateRepository,
    providers::{LocalConfig, LocalProvider},
};

async fn run_workload(
    state_directory: PathBuf,
    workspace_directory: PathBuf,
    supervisor_binary: PathBuf,
) -> Result<()> {
    let state = Arc::new(FileStateRepository::new(state_directory)?);
    let mut local = LocalConfig::trusted_host(workspace_directory);
    local.supervisor = supervisor_binary;
    let sdk = WorkspaceSdk::builder(state)
        .provider(Arc::new(LocalProvider::new(local)?))?
        .build()?;

    let request = CreateRequest::new(WorkspaceId::new("experiment-1")?);
    let workspace = sdk.create(&ProviderId::new("local")?, request).await?;
    let run = workspace.start(CommandSpec::new("git").args(["--version"])).await?;
    let reference = run.reference(); // Persist this value to reconnect later.
    let _ = reference;
    Ok(())
}
```

Storage roots are explicit configuration. For Boat, construct `BoatCreateOptions::new(target_root)` (or `BoatAttachOptions` for an existing Sandbox) and configure a `BoatProvider` with your own Boat client and supervisor deployment. Remote adapters use either a preinstalled supervisor or an explicitly supplied binary compiled for that target’s operating system and architecture.

With the optional `space` feature, `SpaceProvider` attaches to an existing SPACE
sandbox using explicit configuration and a preinstalled supervisor. See the
[pinned API, ownership and transport limits](docs/space-provider.md).

Requirements: Rust 1.89+ to build, a supported POSIX target for process supervision, and Git for checkpoint operations. The SDK does not require a Rust toolchain on a target using a prebuilt supervisor. Your workload can have its own runtime requirements.

## Features

The default build holds what an application needs to run work on a machine:
providers, `WorkspaceSdk` create and attach, `Workspace` and `OwnedWorkspace`,
runs, terminals, file reads and writes, ports, private state, runtime
preparation and `Machines`. Everything else is opt-in.

| Feature | Default | Adds |
| --- | --- | --- |
| `boat` | yes | The Boat provider (`BoatProvider`) and the reexported `boat_client`. |
| `portability` | no | Workspace portability: checkpoints (`checkpoint` engine), `SyncService` sync and handoff, `RecoveryService`, Git worktree helpers (`git`), `DirectoryObserver` and the file-browser and tree-transfer backend operations (`list_directory`, `read_versioned_file`, `publish_file`, `export_tree`, `import_tree`, `remove_transfer`) with their supervisor operations. |
| `container` | no | The Docker/Podman-compatible `ContainerProvider`. |
| `space` | no | The SPACE attachment provider (`SpaceProvider`). |
| `testing` | no | `MemoryStateRepository`, for tests only. |

The state file has one format in every build: a build without `portability`
keeps saved sync, handoff and recovery records as they are. Build the
supervisor you deploy with `portability`, so it serves clients built with or
without it. A supervisor built without it answers those operations with
`Error::Unsupported`.

## Find the model and API

- [Browsable system map and lifecycle decisions](docs/state/README.md): workspace ownership, execution, sync, runtime preparation, and verification limits.

- [Domain model and context boundaries](docs/domain-model.md): entities, value objects, commands, snapshots, and interfaces.
- [Rust SDK guide](docs/sdk-guide.md): configuration, runs, preparation, sync, handoff, and explicit recovery.
- [Acknowledged workspace continuation](docs/recovery-continuation.md): new cooperative steps in the same retained workspace, with durable run ownership and canonical-state checks.
- [Architecture](docs/architecture.md): dependency direction and the Rust supervisor boundary.
- [Build and distribution](docs/release.md): native artifacts and generated Rust documentation.
- [Native authentication integration](docs/pty-integration.md): session controls, durable PTY ownership and verification limits.
- [Validation evidence](docs/evidence/README.md): current native Rust tests, live Boat checks, and their limits.

Generate browsable API documentation with `cargo doc --no-deps`. Public domain types explain their purpose, fields, lifecycle, and constraints in Rustdoc.

## Implementation scope

| Area | Responsibility |
| --- | --- |
| Providers | Local trusted host, Boat, SPACE attachments, SSH, and Docker/Podman-compatible containers through injected `IProvider` / `IBackend` interfaces. |
| Execution | Durable run identity, duplicate-launch protection, reconnection, bounded output, deadlines, and process-group cancellation. |
| Files and checkpoints | Binary chunks, validated relative paths, filtered snapshots, three-way reconciliation, conflict preservation, and apply recovery. |
| Synchronization | Empty-target initialization, reviewed return sync, and stopped-source handoff with an explicit continuation command. |
| Runtime preparation | Tool/resource checks, explicit installation/setup recipes, credential resolution, and native agent authentication actions. |
| Persistence | Typed repository records, stable references, atomic file storage, and workflow leases. |

Native agent login and inference require the user’s own authentication. Portable file handoff does not imply portable VM memory or automatic conversion of agent conversation stores. Provider deletion is asynchronous where appropriate; acceptance is not proof of completed cleanup.

The unreleased 0.2 API replaces the original prototype. Registry publication remains disabled until the vendored Boat dependency has a distributable registry release. The current delivery is the Rust source library and native supervisor; additional language bindings are deferred.

## Development

```sh
cargo test --locked --all-features --all-targets
cargo clippy --locked --all-features --all-targets -- -D warnings
cargo fmt --check
cargo doc --locked --no-deps
```

Related domain concepts share focused files inside context modules. Module entry files declare and export their contents. Unit tests sit beside the behavior they verify; integration tests use Rust workload fixtures.

Run `scripts/quality-gate.sh` for the mechanical and local performance checks.
Acceptance additionally requires separate independent code-quality/design and
adversarial agent reviews under [the coding standards](coding_standards.md).

The [file browser contract](docs/state/files.md) covers the qualified first slice: bounded directory pages, revision-bound binary reads, caller-driven reconciliation and exclusive creation on Linux. Portable observations also target macOS. Unattended live replacement and automatic collaborative text merging remain outside this scope.

See [recovery at an expected checkpoint](docs/recovery-expected-head.md) for exact-head admission and retry behavior.

See [retained byte output](docs/run-output-bytes.md) for binary-safe paging and retention limits.

See [retained fingerprint verification](docs/retained-fingerprint-inspection.md) for proof-only inspection and engine compatibility.
