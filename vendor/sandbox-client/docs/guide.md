# Rust SDK guide

`boat_client` is an asynchronous client for the Boat API. Use it to create and
operate cloud sandboxes, execute commands, read and write files, run agent prompts,
consume events, and manage snapshots and environments.

`BoatApi` is the entry point. Its methods return typed `Result<T>` values and use a
shared connection pool. Cloning the client is inexpensive.

## Install and configure

Requires Rust 1.89 or newer.

```toml
[dependencies]
boat_client = { git = "https://github.com/nessalabs/boat-client-rs" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
futures-util = "0.3" # StreamExt for event streams
```

Configure an API key from the Sandbox dashboard or CLI:

```bash
export BOAT_API_KEY=sandbox…
# Optional: BOAT_ORG, BOAT_BASE_URL
```

```rust,no_run
use boat_client::{BoatApi, BoatClientConfig, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let api = BoatApi::new(BoatClientConfig::from_env()?)?;
    let sandboxes = api.sandboxes(None).await?;
    for item in sandboxes.sandboxes {
        println!("{}: {}", item.id, item.state.as_str());
    }
    Ok(())
}
```

You can also construct `BoatClientConfig::new(api_key)` directly and use its builder
methods to set the organization, base URL, timeouts, and response-size limit.

## Create a Sandbox, run commands, and use files

This example creates a Sandbox with a 30-minute lifetime, waits for it to become
available, writes a file, runs a command, and reads the result. It requests
stop/archive even if one of those operations returns an error. Archive retains
snapshots for later resume.

```rust,no_run
use boat_client::{
    read_text, wait_until_ready, write_text, BoatApi, CreateSandboxRequest, Result,
};

async fn run_job(api: &BoatApi) -> Result<String> {
    let created = api.create(Some(CreateSandboxRequest::ttl(1800)), &Default::default()).await?;
    let sandbox_id = &created.sandbox.id;

    let result = async {
        wait_until_ready(api, sandbox_id).await?;
        write_text(api, sandbox_id, "/tmp/input.txt", "hello from Rust").await?;
        let command = api.exec(sandbox_id, "cat /tmp/input.txt > /tmp/output.txt").await?;
        println!("exit code: {:?}", command.exit_code);
        read_text(api, sandbox_id, "/tmp/output.txt").await
    }.await;

    let stopped = api.stop(sandbox_id, None).await;
    stopped?;
    result
}
```

`exec` waits for a command result. For a custom directory or timeout, use
`CommandRequest::new(command).with_cwd(path).with_timeout_seconds(seconds)` with
`command`. For background execution, set `detached = Some(true)` and call
`command_raw`. It returns `CommandResult::Started` with a process ID; query its
logs and completion through `command_status`.

`read_file` and `write_file` expose file content and encoding directly.
`snapshot_file` and `artifact` return binary `Vec<u8>` values.

## Run a prompt and consume events

`prompt` queues work for a configured provider. `wait_for_prompt` returns a run
once it reaches `finished` or `failed`. Use `stream_prompt` to consume that
prompt's events as they arrive:

```rust,no_run
use boat_client::{
    stream_prompt, BoatApi, CancellationToken, PromptProvider, PromptRequest,
    Result, StreamPromptOptions,
};
use futures_util::{pin_mut, StreamExt};

async fn run_prompt(api: &BoatApi, sandbox_id: &str, cancellation: CancellationToken) -> Result<()> {
    let request = PromptRequest::new(PromptProvider::Codex, "Inspect the project and summarize it.");
    let events = stream_prompt(api, sandbox_id, request, Some(StreamPromptOptions {
        cancellation: Some(cancellation),
        ..Default::default()
    }));
    pin_mut!(events);

    while let Some(event) = events.next().await {
        let event = event?;
        println!("{}", event.type_);
        // Inspect event.data deliberately when you need the event payload.
    }
    Ok(())
}
```

Call `cancel()` on a clone of the token to interrupt local requests, sleeps, or
consumption of buffered events. Cancellation returns `Error::Aborted` and cannot
undo a prompt already accepted by the server. Dropping the stream stops local
work too.

Streams start on their first poll and fetch one page at a time. `stream_events`
defaults to new events only; use `StreamEventsOptions { include_existing: true,
..Default::default() }` to include history. It supports event-type filters and a
page-size limit. `events` exposes an individual page and its cursor directly.

Prompt streams filter events by the queued prompt's task ID and drain pages
before completion. A terminal run status triggers one final refresh to collect
recently persisted events. Repeated or unusable cursors return an error before
replaying a page. Events persisted after the final refresh are outside that
stream's observation window.

## Snapshots and environments

List snapshots globally or for a Sandbox. Follow `page_info.next_cursor` when
`page_info.has_more` is true:

```rust,no_run
use boat_client::{BoatApi, Result, SnapshotsQuery};

async fn inspect_snapshots(api: &BoatApi, sandbox_id: &str) -> Result<()> {
    let first = api.list_sandbox_snapshots(sandbox_id, Some(&SnapshotsQuery {
        limit: Some(20),
        sort: Some("desc".into()),
        ..Default::default()
    })).await?;

    for snapshot in first.snapshots {
        println!("{}", snapshot.id);
    }
    if let Some(page) = first.page_info.filter(|page| page.has_more) {
        let next = api.list_sandbox_snapshots(sandbox_id, Some(&SnapshotsQuery {
            limit: Some(20),
            sort: Some("desc".into()),
            cursor: page.next_cursor,
        })).await?;
        println!("{} snapshots on the next page", next.snapshots.len());
    }
    Ok(())
}
```

`snapshot_tree` inspects a snapshot's files. `snapshot_file` downloads a file or
folder, and `snapshot_download` returns the reconstruction manifest. Named
snapshots have separate save, list, get, and delete methods.

Environment methods manage reusable variables, secret files, repositories, and
versions. For example:

```rust,no_run
use boat_client::{BoatApi, Result, SetEnvironmentVarRequest};

async fn configure_environment(api: &BoatApi, environment_id: &str) -> Result<()> {
    api.set_environment_var(environment_id, "APP_MODE", SetEnvironmentVarRequest {
        value: "development".into(),
    }).await?;
    Ok(())
}
```

The client also provides webhook management, API-key usage, repository selection,
account limits, secrets, and data-retention policy methods. `update_secrets`
replaces the secret configuration, so include every value that should remain.
Changing data retention requires an interactive session token.

## Permanent deletion

`stop` archives a Sandbox for later resume. `delete_sandbox` permanently deletes it and
returns an accepted background operation. The client supplies the required
confirmation header. Observe that operation to confirm cleanup:

```rust,no_run
use boat_client::{wait_for_deletion, BoatApi, Result, WaitOptions};

async fn delete_and_wait(api: &BoatApi, sandbox_id: &str) -> Result<()> {
    let accepted = api.delete_sandbox(sandbox_id).await?;
    let operation = wait_for_deletion(api, &accepted.operation.id, Some(
        WaitOptions::from_millis(900_000, 5_000),
    )).await?;
    println!("deletion {} completed", operation.id);
    Ok(())
}
```

Only `completed` proves cleanup finished. A Sandbox can disappear from normal reads
while its deletion remains `pending`, `processing`, or `blocked`. The default
five-minute wait is a caller budget; Sandbox documents no completion deadline or
blocked-state retry schedule. Choose a longer timeout, zero for unlimited
observation, or check `get_deletion_operation` later. See the
[Sandbox deletion documentation](https://docs.boat.dev/data-retention).

## Options and errors

Request structs use `Option` for optional fields and snake_case Rust names for
JSON fields. Nullable settings such as TTL, webhook names, and prompt model or
reasoning options use nested `Option`:

| Rust value | Request behavior |
| --- | --- |
| `None` | Omit the field |
| `Some(None)` | Send JSON null |
| `Some(Some(value))` | Send the value |

Dates are ISO strings. Extensible response statuses and kinds remain strings;
models with `extra` retain unrecognized fields. Response `Option` values generally
combine absent and null. IDs, counts and byte sizes use integer types, while
fractional balances and usage use `f64`.

Every API call returns `boat_client::Result<T>`. Inspect `Error::status()`,
`api_message()`, and `api_details()` for a server failure. Non-envelope HTTP errors
use `Error::HttpStatus`; decoding errors use `Error::Serde`. An HTTP 2xx response
still preserves the payload's `ok` field, including `false`.

Error formatting omits server messages, details, raw bodies, and decoding values.
Transport errors strip request URLs. Content-bearing model fields are redacted
from `Debug`; deliberate field access still exposes their values.

## Timeouts and retries

| Setting | Default |
| --- | --- |
| Connect timeout | 10 seconds |
| HTTP request timeout | 60 seconds |
| Command timeout through `exec` | 30 seconds |
| Command HTTP timeout | Command timeout + 15 seconds, at least the request timeout |
| Ready / desktop / deletion wait | 5 minutes |
| Idle wait | 10 minutes |
| Prompt wait / stream | 30 minutes |
| Event stream | Unlimited |
| Wait polling interval | 2 seconds |
| Stream polling interval / page size | 1 second / 100 events |
| Buffered response limit | 64 MiB |

Nonzero wait budgets include requests, retries, sleeps, and buffered stream
consumption. A zero timeout means unlimited observation. Cancellation is
cooperative; synchronous JSON parsing is not a real-time scheduling guarantee.

By default, GET calls make at most three attempts for connection/timeouts, HTTP 429/502–504,
or 409 `box_starting` / `box_securing`. Backoff includes jitter and respects
`Retry-After`. A requested delay longer than the HTTP request timeout returns the
error without retrying early. Waits and streams propagate exhausted errors.

Mutations are never automatically retried. For your own retry handling, supply
the idempotency key in [`CreateOptions::idempotency_key`] when calling
[`BoatApi::create`], or as the third argument (`Some("your-stable-key")`) to
[`BoatApi::fork`]. Responses above the configured buffer limit return
`Error::ResponseTooLarge`, including errors and chunked downloads.


## Configure retries and logging

`BoatClientConfig` is the public configuration type. `BoatApi::config()` exposes
the active configuration.
Retry settings apply to GET requests; mutations and handoff action callbacks are
never automatically retried.

```rust,no_run
use std::time::Duration;
use boat_client::{BoatApi, BoatClientConfig, RetryConfig, Result};

fn client() -> Result<BoatApi> {
    let config = BoatClientConfig::from_env()?
        .with_retry(RetryConfig {
            max_attempts: 5,
            initial_delay: Duration::from_millis(250),
            max_delay: Duration::from_secs(10),
            multiplier: 2,
            jitter: true,
        });
    BoatApi::new(config)
}
```

| Retry setting | Default | Meaning |
| --- | --- | --- |
| `max_attempts` | 3 | Includes the first request; 1 disables retries |
| `initial_delay` | 200 ms | Base delay before the first retry |
| `multiplier` | 2 | Multiply the base delay for each subsequent retry |
| `max_delay` | 30 s | Cap on local backoff, including jitter |
| `jitter` | true | Add a random delay from zero through the current base delay |

Without jitter, delays are `initial_delay × multiplier^retry_index`, capped at
`max_delay`. With jitter, the final delay is also capped. `max_attempts` and
`multiplier` must be at least 1, and `initial_delay` cannot exceed `max_delay`.
Invalid settings return `Error::Config` when constructing `BoatApi`.

The server's `Retry-After` is a minimum and may exceed `max_delay`. If the final
wait would exceed `request_timeout`, the client returns the error. Request
timeouts apply per attempt; they are not a total retry budget. Use an outer
`tokio::time::timeout` for a total call budget, or the built-in wait/stream
budgets for those helpers.

### Logging belongs to the application

The SDK emits structured DEBUG events and request spans through `tracing`.
It does not install a subscriber, parse logging environment variables, or add a
logging switch to `BoatClientConfig`. This follows the
[tracing guidance for libraries](https://docs.rs/tracing/latest/tracing/#in-libraries).

To select logging with `RUST_LOG`, add this dependency to your **application**:

```toml
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

Initialize your subscriber once at application startup:

```rust,no_run
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("warn")))
        .init();
    // Start your application and use BoatApi normally.
}
```

```bash
RUST_LOG=boat_client=debug cargo run          # SDK diagnostics
RUST_LOG=boat_client::retry=debug cargo run   # Retry events only
RUST_LOG=boat_client=off cargo run            # Disable SDK diagnostics
```

Applications with an existing subscriber keep their existing setup. They can
also set filters in code, use JSON output, or export tracing spans. Without a
subscriber collecting the events, the SDK does not print logs.

Targets are `boat_client::http`, `boat_client::retry`, and `boat_client::handoff`.
Request spans identify a static operation name and HTTP method. Events report
attempts, status, elapsed time, retry delay, and a fixed error category. Handoff
events report phase and revision. The SDK does not log URLs, resource IDs,
organization IDs, idempotency keys, tokens, headers, bodies, checkpoint references,
server request IDs or server error text, even at TRACE level. Select SDK targets
when you want SDK diagnostics; other dependencies control their own logging.

## Handle exhausted errors in your scheduler

`Error::is_retryable()` classifies transient failures; it does not mean repeating
a mutation is safe. `Error::Api` contains `status`, `code`, `request_id`, `message`,
`details`, and `retry_after`. `details` is boxed to keep the error enum compact;
`api_details()` still returns `Option<&serde_json::Value>`. Non-envelope HTTP errors use `Error::HttpStatus`
with `status`, truncated `body`, and `retry_after`. Accessors include `status()`,
`api_message()`, `api_details()`, `unexpected_body()`, and `retry_after()`.

`retry_after()` preserves valid seconds or HTTP-date headers even when retries
are disabled/exhausted, or the failed call was a mutation. Dates are converted
to a delay when the response arrives. Missing or invalid headers return `None`.
The value is not a countdown; callers persisting it should record their own
observation time. Timeout/connect errors use `Error::Http`; decoding, oversized
responses, cancellation, and terminal Sandbox states have their own variants.

If your scheduler owns retries, use `RetryConfig::disabled()` to avoid multiplying
its attempts by the SDK's attempts. Inspect a safe GET failure like this:

```rust,no_run
use boat_client::{BoatApi, BoatClientConfig, RetryConfig, Result};

async fn inspect() -> Result<()> {
    let api = BoatApi::new(BoatClientConfig::from_env()?
        .with_retry(RetryConfig::disabled()))?;
    match api.sandboxes(None).await {
        Ok(response) => println!("sandboxes={}", response.sandboxes.len()),
        Err(error) if error.is_retryable() => {
            // Pass this metadata to your scheduler; no retry is launched here.
            eprintln!("transient status={:?}, retry_after={:?}",
                error.status(), error.retry_after());
            return Err(error);
        }
        Err(error) => return Err(error),
    }
    Ok(())
}
```

## Durable handoff progress

The public `handoff` module provides `HandoffJournal`, a pluggable `ReceiptStore`,
and a local Unix `FileReceiptStore`. The journal persists checkpoint references
and capture/restore/start intent and confirmations. Your application supplies
the actual transfer and continuation actions through callbacks or explicit
transitions. Recovery checks ambiguous outcomes before repeating work. See the
module documentation for configuration, recovery examples, and storage guarantees.
