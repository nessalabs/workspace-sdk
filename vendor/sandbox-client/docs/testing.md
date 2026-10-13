# Testing

## Offline checks

```bash
cargo fmt -- --check
cargo test
cargo clippy --all-targets -- -D warnings
git diff --check
```

These commands work in a fresh checkout: this library intentionally does not
track `Cargo.lock`, so do not add `--locked` before generating one. CI also checks
Rust 1.89 and reruns the Node fixture verification against the pinned SDK.

Unit tests cover configuration, validation, error formatting and core models.
`tests/api_contracts.rs` covers endpoint and convenience-helper behavior.
`tests/runtime_regressions.rs` adds targeted tests for runtime edge cases, using
non-sensitive fixtures under `tests/fixtures/`. `tests/sdk_operations.rs` checks all
59 endpoint operations with full/minimal requests and responses, API failures and non-retried
mutations. `tests/streams.rs` covers cursor advancement, page draining, cancellation
and deadlines. `tests/error_contracts.rs` checks non-JSON failures, decoding
redaction, chunked response caps and nullable/fractional wire values. No credential
is needed.

To verify fixture provenance, unpack the reference package `@boatdev/sdk@1.9.7` and run:

```bash
node scripts/sdk_fixtures.cjs /path/to/unpacked/package
node tests/verify_ts_contract.cjs /path/to/unpacked/package
```

Regenerate the checked-in operation fixtures by adding `--write` to the first
command, then review the diff.

The Node scripts use the package's actual converters and request builders. They
check nested wire values and request options without making API requests. The
package path is an argument; no machine-specific path or dependency is checked in.

## Live checks

Each live test is ignored by default. Supply a short-lived `BOAT_API_KEY` through
the process environment; never save it in source, fixtures or output artifacts.
Run a named test so mutation requirements are explicit:

```bash
# Account and list smoke
cargo test --test live live_me_and_boxes -- --ignored
# Read-only environment, snapshot tree and download-manifest contracts
cargo test --test live live_runtime_read_contracts -- --ignored
# Existing operable box: requires SANDBOX_ID; runs a harmless shell command
cargo test --test live live_get_and_exec_existing_box -- --ignored
# Creates, updates and deletes only its own uniquely named environment
BOAT_TEST_MUTATIONS=1 cargo test --test live live_environment_round_trip -- --ignored
# Creates one box; verifies Rust/SDK PUT/read parity; deletes it and waits for completion
BOAT_TEST_MUTATIONS=1 BOAT_SDK_PATH=/path/to/unpacked/package cargo test --test live live_sdk_file_round_trip -- --ignored
```

`live_runtime_read_contracts` reads at most one snapshot tree/download manifest and
prints counts only. Without a snapshot fixture those routes are skipped. The
environment mutation test cleans up its fixture even if the update/read checks
fail, then verifies that the environment is absent. The file round-trip test creates one disposable box with a five-minute TTL, writes
a fixed scratch path inside that box, reads/writes through both clients, and waits
for its permanent deletion even if the file check fails. It prints the file
result before observing cleanup so a deletion timeout cannot hide that outcome.
The cleanup wait defaults to five minutes: this is a test budget, not a Sandbox
completion guarantee. A documented `blocked` state may outlast that budget and
fail the live test without proving either an SDK mismatch or a service defect.
Use the returned operation ID to check again; only `completed` confirms cleanup.
See [Sandbox data-retention and deletion](https://docs.boat.dev/data-retention).
No live test deletes a pre-existing snapshot.

## Remaining verification scope

The fixed encoding and typed models were checked against the live environment
update/delete lifecycle and a nonempty snapshot tree/download manifest. These are
smoke checks, not load benchmarks or a complete state-space test. Provider-backed
prompt completion, full desktop availability, large output recovery and actual
multi-page events need suitable live fixtures. Offline tests cover those modeled
contracts and the shared polling logic described in the SDK guide.

For load testing, use a dedicated organization and bounded disposable resources;
measure command-log transfer, response size, latency, cancellation and rate-limit
behavior. Check `limits()` before creating compute resources. No stress-create
loop is part of the normal test suite.


## Retry and recovery tests

`tests/retries.rs` verifies configurable attempts, disabling retries,
`Retry-After` metadata, mutation safety, and structured opt-in logging without
sensitive values. Backoff bounds and validation have unit coverage.
`tests/handoff.rs` covers durable restarts, stale/concurrent updates, lost write
acknowledgements, cancellation after commit, cross-process locking, corrupted
records, permissions, size limits, case-distinct IDs, and application callbacks.
The ignored child lock probe is run automatically by its parent test.

Two explicit live probes are available in `tests/live_recovery.rs`:

```bash
# Inject two local 503 responses, then perform an authenticated live read.
cargo test --test live_recovery live_read_after_injected_transient_failures -- --ignored --nocapture

# Create two fixture Sandboxes, copy a counter checkpoint through application code,
# reconcile after restore and launch, then delete both and check completion.
BOAT_TEST_MUTATIONS=1 BOAT_TEST_STATE_DIR=work/live-recovery \
  cargo test --test live_recovery live_handoff_reconciles_completed_launch_and_cleans_up -- --ignored --nocapture
```

Both require `BOAT_API_KEY`. The mutation probe stores resource IDs, idempotency
keys, receipts, and its non-sensitive checkpoint in the chosen state directory.
It uses a five-minute Sandbox lifetime, no inherited environment, and never touches
existing Sandboxes. Functional and cleanup outcomes are printed separately. The test
fails if deletion has not completed within its two-minute observation budget;
`blocked` is a valid service state, so this alone does not diagnose a backend bug.
Use the saved deletion IDs to continue checking without creating replacement
resources. Do not remove the state directory until cleanup is confirmed.

## Boat migration evidence

Live checks described above were recorded against the former Box deployment.
They are historical evidence, not a live Boat certification. The Boat migration
uses published OpenAPI/TypeScript SDK 1.9.7 contracts, offline HTTP fixtures,
independent boundary tests and scoped loopback measurements. No Boat cloud
resources or real credentials were used. The 59 supported operation fixtures
exercise JSON commands (`stream: false`); separate real NDJSON mocks prove that
unsupported synchronous streaming is rejected before any request is delivered.
