# Boat Rust SDK

An asynchronous Rust client for the [Boat API](https://docs.boat.dev/api/v1).
Create cloud sandboxes, execute commands, read and write files, run agent prompts,
stream events, and manage snapshots, environments, and account settings.

The crate is `boat_client` and requires Rust 1.89 or newer.

## Install

```toml
[dependencies]
boat_client = { git = "https://github.com/nessalabs/boat-client-rs" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

## Quick start

Set `BOAT_API_KEY` to a key from the Boat dashboard or CLI. `BOAT_ORG` and
`BOAT_BASE_URL` are optional.

```rust
use boat_client::{BoatApi, BoatClientConfig, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let api = BoatApi::new(BoatClientConfig::from_env()?)?;
    let response = api.sandboxes(None).await?;

    for item in response.sandboxes {
        println!("{}: {}", item.id, item.state.as_str());
    }
    Ok(())
}
```

## Documentation

- [SDK guide](docs/guide.md): complete Rust examples for commands, files, prompts,
  streams, snapshots, environments, cleanup, and error handling.
- [API reference](docs/api.md): all 59 endpoint operations and convenience helpers.
- [Handoff receipts](docs/handoff.md): durable progress records, checkpoint references,
  application callbacks, and configurable storage.
- [Testing](docs/testing.md): local checks, fixture verification, and live tests.

Build searchable API documentation with method signatures and request/response
types:

```bash
cargo doc --no-deps --open
```

## Main APIs

| Area | Entry points |
| --- | --- |
| Sandboxes | `sandboxes`, `create`, `get`, `update`, `stop`, `resume`, `fork`, `delete_sandbox` |
| Commands | `exec`, `command`, `command_raw`, `command_status`, `interrupt` |
| Files | `read_file`, `write_file`, `read_text`, `write_text`, `artifact` |
| Prompts and events | `prompt`, `prompt_run_status`, `events`, `stream_prompt`, `stream_events` |
| Desktop and access | `desktop`, `host_port`, `ssh_key` |
| Snapshots | `list_snapshots`, `list_sandbox_snapshots`, `snapshot_tree`, `snapshot_file`, `snapshot_download` |
| Named snapshots | `save_named_snapshot`, `get_named_snapshot`, `list_named_snapshots`, `delete_named_snapshot` |
| Environments | Environment lifecycle, variables, secret files, repositories, and upgrades |
| Account | Limits, API-key usage, webhooks, repositories, secrets, and retention policy |
| Waiting | `wait_until_ready`, `wait_until_idle`, `wait_for_prompt`, `wait_for_desktop`, `wait_for_deletion` |

## Runtime behavior

The client shares a connection pool across clones. Streams are lazy and support
`CancellationToken`. Waits bound requests, retries, and sleeps; zero timeout means
unlimited observation.

GET requests retry transient failures up to three attempts by default and honor
`Retry-After`. `BoatClientConfig` exposes attempts, exponential backoff and jitter.
Diagnostics use application-filtered `tracing`.
Mutations are never automatically retried. Responses have a configurable 64 MiB
buffer limit. Error formatting and content-bearing model `Debug` output redact
sensitive data.

Permanent deletion returns an accepted operation. Only `completed` confirms
cleanup; a hidden Sandbox or a `blocked` operation does not. The deletion wait timeout
is a caller budget, not a service completion guarantee.

## Examples and checks

```bash
cargo run --example list_sandboxes
RUST_LOG=boat_client=debug cargo run --example logging
SANDBOX_ID=bx_… cargo run --example exec_smoke
SANDBOX_ID=bx_… BOX_PROMPT='Summarize the project' cargo run --example stream_prompt

cargo test
cargo clippy --all-targets -- -D warnings
```

## License

MIT

## Service naming and contract scope

The stable implementation entry points are `Api` and `ClientConfig`; `BoatApi`
and `BoatClientConfig` are public aliases declared in `src/lib.rs`. Service URL,
credential environment variables, organization header and user-agent defaults
live in `src/service.rs`. Resource models use the neutral term `Sandbox`.
Protocol names remain distinct from branding: Boat retains `bx_` resource IDs,
`X-Ascii-Confirm-Delete`, webhook signing headers and the currently documented OAuth scopes.
Handoff receipts use `source_sandbox_id`/`target_sandbox_id`; obsolete Box
identity fields are rejected. No Box API, environment-variable or receipt aliases
are supported.

This migration verifies the existing 59-operation surface against the published
`@boatdev/sdk@1.9.7` request builders and response converters. The explicit
inventory is `scripts/supported-operations.json`; newer upstream operations are
not advertised as implemented. Environment operations use the configured `org`
as a query scope. Clone the client with `with_org(Some(org))` for a distinct
organization. `with_org(None)` clears explicit scope: environment operations
use personal scope, while sandbox operations use the server's active wallet.
Use `with_org(Some("personal".into()))` for explicit personal billing.

Run the scoped loopback qualification with
`cargo test --test client_performance -- --ignored --nocapture`. Its three
samples check 384 exact response bodies and request counts; they do not qualify
WAN latency, live cloud behavior, RSS or statistical regressions.

Synchronous command methods support JSON responses. `stream: true` requires an
NDJSON decoder and is rejected before HTTP delivery; no command is launched by
that rejected call. Detached commands retain the server's documented behavior
of ignoring `stream`. Canonical SDK parity fixtures exercise `stream: false`;
independent NDJSON regressions verify rejection and zero requests.
