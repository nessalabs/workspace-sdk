# Build and distribute the Rust SDK

The current deliverables are the Rust library source and `workspace-supervisor`. There is no TypeScript package or client JSONL executable. The native supervisor is infrastructure used by the library’s provider adapters.

## Local build

```sh
cargo build --locked --no-default-features --bin workspace-supervisor
cargo test --locked --all-features --all-targets
cargo clippy --locked --all-features --all-targets -- -D warnings
cargo fmt --check
cargo doc --locked --no-deps
```

Generated API documentation is in `target/doc/workspace_sdk/index.html`. Domain documentation is attached to the Rust definitions, so it travels with generated Rustdoc rather than existing only in a separate guide.

Install the supervisor into a caller-chosen prefix:

```sh
cargo install --locked --path . --no-default-features --bin workspace-supervisor --root "$INSTALL_PREFIX"
```

Set `INSTALL_PREFIX` to an application-owned directory first. Configure `LocalConfig.supervisor` with the resulting executable path, or install it on the target and use `SupervisorDeployment::Preinstalled`.

## Remote artifacts

A target needs a supervisor built for its operating system and CPU architecture. Build natively on that platform or use a configured Rust cross-compilation toolchain, then supply its path and `TargetPlatform` in `SupervisorArtifact`. For Linux, choose a C-runtime-compatible build; matching CPU architecture alone does not guarantee that a dynamically linked binary runs in every container image.

`SupervisorDeployment::Upload` requires an explicit target executable path and compatible local artifact. It does not guess a remote home directory, silently copy the controller’s executable, or install a Rust compiler. Application build/release infrastructure owns artifact production and distribution.

The Boat client is vendored under `vendor/sandbox-client` with provenance. Source consumers do not need a sibling checkout. Registry publication is still disabled pending a distributable registry release of that dependency; do not describe local builds as a published crate.

## Scope of validation

The current native Rust implementation passed 93 unit/integration tests, three documentation tests, strict Clippy, formatting, and seven live Box workflow checks. The native Linux supervisor and runnable local example also built and ran successfully. Both live fixtures accepted stop/delete requests, but provider deletion remained blocked. Exact receipts and native binary fingerprints are in the [current evidence](evidence/native-rust-box-live.json).

Run local native-supervisor and checkpoint tests for any supervisor/protocol change. Remote deployment additionally needs validation against the target environment. Historical Python-supervisor and TypeScript test results are retained only as historical evidence and do not establish correctness of the current Rust implementation. See [evidence](evidence/README.md) for recorded live cleanup limitations.
