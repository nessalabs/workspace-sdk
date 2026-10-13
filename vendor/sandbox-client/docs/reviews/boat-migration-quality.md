# Boat migration independent quality and performance review

Reviewer: performance_gate. Compared the actual working diff to 69ca3c9;
reviewed transport, configuration, response/request models, helper/wait changes,
Handoff compatibility, SDK-generated fixtures, boundary tests and loopback
measurement. No reviewer builds or benchmark runs.

## P2: command streaming option has no matching response decoder

New `CommandRequest.stream` can be set to true and is sent by `command_raw`
(src/client.rs, command_raw). Published @boatdev/sdk 1.9.7
`src/models/CommandRequest.ts:47` documents that this changes the response to
application/x-ndjson, one CommandStreamFrame per line, unless detached is true.
The Rust path still calls the generic send decoder, which applies
serde_json::from_slice to the entire response as CommandResult. A real streaming
command can execute successfully and then report a decode error. The current
59-operation contract fixture sets stream=true and detached=false but returns
a single synthetic CommandResponse JSON, so its green status does not prove
this newly exposed option works.

Implement the streaming response contract, or reject unsupported non-detached
streaming before any request is sent and document that boundary. Tests must
exercise the actual selected response mode or enforce pre-dispatch rejection.
Do not treat the current synthetic JSON response as streaming support. This
P2 blocked the initial review; the completed re-review below resolves it.

## Architecture and other reviewed boundaries

Neutral Api/ClientConfig own implementation; Boat aliases introduce no wrapper
state or duplicate behavior. Service constants centralize branding and the
resource prefix without rewriting stable wire identities. Transport retains
pooled-client ownership, bounded response reads, GET-only automatic retries and
redacted diagnostics. The environment-specific query organization scope is
encoded once at request construction, with boundary tests proving special
characters round-trip and sandbox scope continues in X-Boat-Org.

Configuration checks parsed host identity rather than a numeric string prefix,
and Api::new repeats validation because fields are public. The retained boundary
RED demonstrates the constructor bypass before the fix. Readiness now treats
cancelled as terminal and supports the documented minimal id/state/error
response through optional name. Nullable request fields distinguish omission
from explicit null. HandoffPlan changes Rust field names but explicitly retains
source_box_id/target_box_id serialized keys; the schema-v1 round-trip regression
asserts exact persisted JSON equality. No dependency-direction or ownership
problem found in these reviewed changes.

Nit: error Display strings still say box after renaming public variants to
Sandbox. The SDK fixture generator also retains a dead model-name Box branch.
These are low-priority consistency items, separate from the stream blocker.

## Evidence inspected

The completed boat-final logs show successful all-target tests, Clippy, 12
documentation tests, 59 SDK operation comparisons and 13 runtime fixture
comparisons. The SDK inventory is explicit and checked for exact membership;
this is a supported subset, not the full current API. Request assertions retain
method, path, query, headers, body and mutation request-count checks. Response
comparison intentionally normalizes optional nulls; explicit nullable request
and cancelled response tests supplement it. Live ignored tests are not a
service validation pass. The stream mismatch above remains despite these greens.

The explicitly invoked performance test passes, sends exactly 384 HTTP requests
through one client across three samples of 128 sequential loopback reads, and
asserts every decoded id/name/state, authorization header and empty request
body. Reported samples are 19.561833, 16.715583 and 14.153625 ms on macOS
aarch64. This uses a debug test build and mock server with real loopback HTTP.
It verifies one client is reused; it does not instrument socket reuse or prove
connection-pool hit counts. The ten-second bound is a completion watchdog, not
a speed or regression threshold. No WAN, cloud, RSS, baseline comparison or
statistical performance claim is supported. The workload is honest within
these limits; no performance issue found in the migration's reviewed hot paths.

## Stream P2 re-review: resolved

Read the actual command_raw guard: stream=true is rejected with Config before
request construction whenever detached is absent or false. command delegates
to that same boundary. Detached=true still sends one request and decodes the
started JSON variant, matching the published rule that detached ignores stream.
The guard is owned by the method that promises a JSON result; it adds no
transport-wide policy or duplicate state. README and method documentation
state that NDJSON streaming is unsupported and no remote command is launched
by a rejected call.

The independent command-stream regressions retain a real NDJSON response if
a request escapes. Both command and command_raw tests explicitly require zero
received requests for absent and false detached flags; separate tests preserve
the true-detached started response and explicit-false synchronous response.
Retained RED shows requests escaped before the guard; the completed
boat-stream-green log passes all four stream tests, seven boundary tests and
59 operation tests. A separate boundary check also asserts zero requests.

The generated supported-command fixture now sets stream=false and passes that
value through the real TypeScript request builder. No operation, stream field,
request assertion or response assertion was removed; the same 59 operations
remain checked. Unsupported streaming is covered explicitly rather than hidden
behind a synthetic single-JSON streaming response. The stale Box model-generator
branch is corrected to Sandbox. Cleared organization scope documentation now
distinguishes the active sandbox wallet from personal environment scope, and
the boundary test verifies clone independence, absent scope and explicit
personal scope on all three routes.

Completed boat-gate-*02 logs pass 166 all-target tests (nine ignored), 12
documentation tests, Clippy and rustdoc, plus 59 real SDK operation comparisons
and 13 runtime fixture comparisons. Scoped performance rerun passes its exact
384-call workload; samples are 19.362417, 16.255625 and 13.804875 ms. This is a
new affected-check run after the fix, not evidence of a speedup over the prior
run. Debug-build, loopback, socket-observability and live-service limitations
above remain.

Final scoped quality and performance review: PASS, no unresolved P1/P2 finding.
The remaining box wording in human-readable error strings is a nonblocking
consistency nit. This review does not claim live Boat qualification, complete
upstream API coverage, or a performance regression threshold.

Final reviewed key-file SHA-256 values:

- `src/client.rs`: `c49298624f8a2cf507696523993ddaa3a9c78e571b19d3c1dead7159e93b04a7`

- `src/config.rs`: `8d1d9e01be1135b09b4472e91a2c0e7880d29dbe9ee17b33f607e3c10b6caf73`

- `src/client/transport.rs`: `bc15a3971c5c4b731b556097e348e7d1e60eab6a49b23c3488212d409d299770`

- `scripts/sdk_fixtures.cjs`: `5c91796418c0cc2b4696d87942ce17971e15f0f88f96e3eb52b64fe55b4e2dbc`

- `tests/boat_command_stream.rs`: `44b016e75deecc9115b51dc0153f45f2db65d6242be7c5a31350e08087131904`

- `tests/boat_boundaries.rs`: `a2d203ac43d0ccabbccc8cf721152144734000ae2a8a38f52055dc6b1d218497`

- `tests/client_performance.rs`: `25e8f73691d05cd10e73c0e9328921812cf621e088d3dd5259f1bf0c5cd26a25`

## Current-only follow-up after explicit compatibility removal

The user explicitly removed the backward-compatibility requirement. This
follow-up supersedes the earlier acceptance of legacy Handoff wire field
retention. Reviewed the small diff from 070431a: HandoffPlan now serializes
source_sandbox_id and target_sandbox_id directly and denies unknown fields.
Obsolete source_box_id/target_box_id fields cannot silently substitute or coexist
with current target identities. No automatic migration, alias, target fallback
or receipt reinterpretation was added. Retaining the existing receipt version
with a deliberately breaking field schema is acceptable within this explicit
current-only, pre-publication scope; old receipts are rejected.

The public boundary test round-trips the complete current receipt and rejects
each obsolete identity field separately and together. Its retained
boat-current-only-red log fails on the former source_box_id requirement (six
other tests pass), and the final suite passes the corrected contract. Existing
Handoff compare/exchange, intent-binding and uncertainty behavior is unchanged.
The retry classifier now accepts only documented boat_starting for HTTP 409;
box_starting and box_securing no longer receive legacy treatment. Checked the
published SDK command documentation for boat_starting and the non-retryable
sandbox_not_ready distinction. GET-only automatic retry ownership is unchanged,
so removing aliases cannot introduce mutation replay.

Completed boat-current-only logs report 166 passed all-target tests, nine
ignored, 12 passing documentation tests, passing strict Clippy, 59 SDK operation
comparisons and 13 runtime fixture comparisons. The same 384-request loopback
performance scenario passes with samples 18.032458, 17.453584 and 13.627917 ms.
No source-level hot-path work was added; these samples retain the previously
stated debug/local/mock limitations and do not establish a speedup or regression
budget. No reviewer build or benchmark rerun was performed.

Final current-only scope: PASS, no unresolved P1/P2 quality or scoped-performance
finding. Historical compatibility claims above describe superseded revisions.

Reviewed follow-up hashes:

- `src/handoff/mod.rs`: `0cc0fab00f7d5156a16560dedc788a21cf1459f74adc63405d0a44ddee1afadd`

- `src/error.rs`: `c86a3be4d8a2fa914f74af8e249bed2e74fba686a71c3429dfb7058dbedcbf03`

- `tests/boat_boundaries.rs`: `9ecec8b5f89ce45eb45ac486db8ca5e45cdc85b8af8bfdb5a12a1b067c356b5a`

- `README.md`: `ebfaaeb5978dc2236090417daef287a6761161e45fa535f1bfc01f655aee8d06`
