# Workspace SDK

A Rust SDK for running workloads on user-owned compute, reconnecting to managed processes, and moving selected project files between machines with conflict review.

The SDK is organized around workspace management, execution, synchronization, and runtime preparation. Your application supplies provider configuration, storage locations, credentials, and its agent loop. The target-side supervisor is also Rust. There is no Python supervisor, TypeScript package, or client JSONL CLI.

## Use the Rust library

Build the native supervisor first:

```sh
cargo build --locked --no-default-features --bin workspace-supervisor
cargo run --locked --no-default-features --example local_workflow -- target/debug/workspace-supervisor
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
    CommandSpec, OpenRequest, ProviderId, Result, WorkspaceId, WorkspaceSdk,
    infrastructure::FileStateRepository,
    providers::{LocalConfig, LocalOpenOptions, LocalProvider},
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

    let request = OpenRequest::new(
        WorkspaceId::new("experiment-1")?,
        &LocalOpenOptions::default(),
    )?;
    let workspace = sdk.open(&ProviderId::new("local")?, request).await?;
    let run = workspace.start(CommandSpec::new("git").args(["--version"])).await?;
    let reference = run.reference(); // Persist this value to reconnect later.
    let _ = reference;
    Ok(())
}
```

Storage roots are explicit configuration. For Box, construct `BoatOpenOptions::new(target_root)` and configure an `BoatProvider` with your own Boat client and supervisor deployment. Remote adapters use either a preinstalled supervisor or an explicitly supplied binary compiled for that target’s operating system and architecture.

Requirements: Rust 1.89+ to build, a supported POSIX target for process supervision, and Git for checkpoint operations. The SDK does not require a Rust toolchain on a target using a prebuilt supervisor. Your workload can have its own runtime requirements.

## Find the model and API

- [Domain model and context boundaries](docs/domain-model.md): entities, value objects, commands, snapshots, and interfaces.
- [Rust SDK guide](docs/sdk-guide.md): configuration, runs, preparation, sync, and handoff.
- [Architecture](docs/architecture.md): dependency direction and the Rust supervisor boundary.
- [Build and distribution](docs/release.md): native artifacts and generated Rust documentation.
- [Native authentication integration](docs/pty-integration.md): session controls, concurrency and pending PTY adapter work.
- [Validation evidence](docs/evidence/README.md): current native Rust tests, live Box checks, and their limits.

Generate browsable API documentation with `cargo doc --no-deps`. Public domain types explain their purpose, fields, lifecycle, and constraints in Rustdoc.

## Implementation scope

| Area | Responsibility |
| --- | --- |
| Providers | Local trusted host, Boat, SSH, and Docker/Podman-compatible containers through injected `IProvider` / `IBackend` interfaces. |
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

## Boat naming

Use `providers::BoatProvider`, `providers::BoatOpenOptions` and
`workspace_sdk::boat_client`. The provider feature is `boat`; obsolete Box feature/type aliases are removed. The implementation lives in `providers/sandbox`, the dependency
is imported internally as `sandbox_client`, and its pinned source lives under
`vendor/sandbox-client`. Branded public aliases and the provider display name
are separate from the neutral implementation.

The default provider registry ID is `boat`. Obtain it through `IProvider::id()`
or set an explicit registration ID with `with_id`. Open options use
`sandbox_id` and `sandbox_type` in both Rust and serialized configuration.
Obsolete Box names and unknown or duplicate fields are rejected. This is the
current-only API; no compatibility layer or automatic record migration is provided.

Historical live evidence predates the Boat migration. Public evidence redacts
live resource IDs and local machine paths; unredacted originals remain private.
Current Boat verification uses offline contract tests and local SDK workflows,
not new cloud fixtures or real agent login. PTY integration remains pending.
