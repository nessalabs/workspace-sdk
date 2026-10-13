# Vercel-hosted Linux PTY qualification — 2026-10-12

[Live PTY evidence](vercel-linux-pty-live.json) qualifies SDK revision
`e3465f9afa71cac37dd23326651c8c6ddf939f45` and the reusable
[three-phase probe](../../examples/pty_live_probe.rs) inside one disposable Vercel
Linux x86_64 VM. This uses LocalProvider and BackendRuntimeHost inside the VM;
it does not certify Boat, SSH/container transport or a production Vercel adapter.
Boat compute was unavailable for this qualification.

Separate completed remote commands started the terminal, reconstructed the host
and session, then canceled and cleaned it. The owner and workload retained the
same boot/PID/start identities, and the duplicate launch key produced one launch.
The child confirmed real foreground terminals, acknowledged input without echo,
and observed the requested 120-column, 40-row size. Exact retained cursors moved
40 → 67 → 67 bytes; cancellation returned code 130 and stable gap-free EOF.
Both owned directories were independently confirmed absent after deletion.

This VM's default command environment supplied UID 1000 with permission-bypass capabilities. The initial
unchanged gate therefore failed its owner-permission-denial fixture. A direct
OS probe confirmed the cause. Removing all capability sets with `setpriv` and
setting `NoNewPrivs=1` restored that test's precondition; no assertion or exclusion
was changed. Both complete SDK and SDK-plus-probe gates then passed, including
four native and sixteen adversarial PTY cases in each feature configuration and
the existing managed-run performance gate. The
[PTY performance samples](vercel-linux-pty-performance.json) cover fifteen sessions
at one- and four-session populations. They measure calls inside the VM, excluding
WAN round trips, and are not a statistical regression baseline.

A deliberate lagging-state case remained a failing verification while cleanup
still stopped and removed its owned workspace. All 281 original tracked source
files matched the committed SDK, and the uploaded probe matched its recorded
SHA-256. Full gate logs and successful-run artifacts were downloaded and verified before stopping the
nonpersistent VM; a separate provider read confirmed `stopped`. Independent
quality and adversarial findings, fixes and acceptance decisions are recorded
under `docs/reviews`. Real account login and long-duration resource behavior
remain outside this qualification.

# Native Rust implementation

The current SDK uses a Rust supervisor and Rust file-tree/runtime helpers. The opt-in [native Boat integration test](../../tests/boat_live.rs) accepts explicit target paths and compatible supervisor/workload artifacts. Current native results are recorded separately from the earlier implementation below.

## Current validation — 2026-09-08 UTC

The Boat dependency was subsequently synced to upstream `main` at `69ca3c913986602ae13a0500157390ac118f599a`, without local patches. The adapter now uses the client's `exec`, `write_text`, `CreateOptions`, and `HostPortRequest` APIs. The live report below predates this dependency update and is not a live certification of that revision. Local HTTP contract tests verify private UTF-8 request delivery, failure cleanup, and explicit private hosting after the update.

Post-update checks passed: the full 93-test SDK suite plus the new private-hosting regression (94 distinct unit/integration tests), three documentation tests, all-target compilation, strict Clippy, and formatting. The pristine upstream client passed 150 unit/integration tests and 12 documentation tests on a serial rerun. Its 30 ms stream-deadline test initially timed out under concurrent build/test load; it passed both in isolation and in the serial suite. No cloud fixtures were created for this dependency update.

[Native Rust Boat evidence](native-rust-boat-live.json) records **seven passing live checks**: Linux supervisor upload and workspace creation, filtered tree initialization, 320 KiB binary chunk transfer, Rust workload reconnect, three-way return sync, Boat-to-Boat handoff, and managed cancellation. The run completed in 490 seconds with synthetic data and no model calls. Its report includes SHA-256 fingerprints of both native binaries.

Both fixtures used a 900-second TTL and accepted stop and deletion requests. Both deletion operations reported **blocked** without further detail; completed deletion is unconfirmed. Their exact references and operation receipts are in the report.

The current implementation also passed **93 Rust unit/integration tests and three documentation tests**, all-feature/all-target Clippy with warnings denied, formatting checks, generated API documentation, and the runnable local Rust workflow. The Linux supervisor built successfully for `x86_64-unknown-linux-musl`. Real SSH/container targets and native agent authentication remain unvalidated live.

The old spike harnesses, TypeScript package, JSONL client CLI, and stale distribution archives were removed. Their historical JSON reports remain useful for provider behavior and cleanup receipts, but their test counts do not describe the rewritten supervisor.

# Boat integration evidence

## Replacement SDK 0.2

Before the native-supervisor rewrite, the SDK workflow probe passed seven live checks through the replacement public API: workspace creation/managed execution, filtered initial upload, 320 KiB chunked binary transfer, reconnect with duplicate-launch prevention, three-way sync-back, Boat-to-Boat handoff, and managed cancellation. The deterministic worker continued from counter 3 to 5. No model calls or agent credential transfers were made.

[SDK workflow evidence](sdk-v0.2-live.json) records both exact fixture references and deletion receipts. Both fixtures had 900-second TTLs; stop and deletion were requested, but deletion operations reported **blocked**, so cleanup completion is not confirmed. This probe exercises real SDK/provider behavior; local automated tests separately cover stale/conflicting edits, tracked-secret exclusions, special files, supervisor loss, interrupted import/apply recovery, and failed receipt persistence.

The [final build](sdk-v0.2-final-build.json) was tested again after the file-layout and recovery changes. The [final live run](sdk-v0.2-final-live.json) passed the same seven functional checks, including the updated Boat transport and ownership checks. Both new fixtures accepted stop/delete and reported blocked cleanup. The build records exact source and binary hashes; the earlier run remains separate provenance.

Local validation passed 86 Rust unit/integration tests, two documentation tests, all-feature Clippy with warnings denied, formatting checks, and the runnable local workflow. Those TypeScript and source-installation checks applied to the former distribution; those artifacts have since been removed. Real SSH/container targets and native model authentication remain outside these live checks.

## Historical primitive probes

The following reports were created before the replacement SDK implementation. They establish provider primitives and explain the client correction; their limitations are specific to those historical runs.

Executed 2026-09-07 America/Vancouver (2026-09-08 UTC). These are live provider probes with synthetic data, using the real Rust client. No model calls were made. The state-handoff test uses a deterministic Python worker.

**Result:** the core execution and checkpoint approach is supported by the probes. The full run passed 17 checks; its hosting check initially failed because the HTTP test client did not retain cookies. A focused browser-style follow-up passed. Background deletion remains blocked at the provider, so the suite is not an all-green production certification.

## Evidence and corrections

| Evidence | Result |
| --- | --- |
| [Original client run](boat-contract-live.json) | Creation, idempotency, working directories, and required tools passed. File writing failed with HTTP 404. |
| [Isolated dependency patch](boat-client-file-write.patch) and [build fingerprint](boat-client-build.json) | Zero-context excerpt of the historical `POST` to `PUT` correction in a copied `boat_client`. The shared dependency checkout was not changed. |
| [First corrected run](boat-contract-corrected.json) | Binary transfer, reconnect, duplicate-payload prevention, and output-tail checks passed. Bundle restoration found a harness bug: its branch needed to be explicit. |
| [Full checkpoint/run suite](boat-contract-final.json) | 17 of 18 checks passed. Includes both file round trips and independent Boat-to-Boat transfer. The original cookie-less hosting assertion failed. |
| [Initial hosting diagnostic](port-diagnostic.json) | The service worked inside the Boat, but cookie-less external requests ended at HTTP 403. |
| [Browser-style hosting follow-up](port-browser-live.json) | Token URL → HTTP 302 with `Set-Cookie` → HTTP 200 and fixture content. A fresh unauthenticated client received HTTP 403. |
| [Final cleanup verification](cleanup-verification.json) | All five disposable fixtures' deletion operations reported `blocked`; normal Boat reads returned 404. Deletion completion is unconfirmed. |
| [Updated Git client build](boat-client-updated-build.json) | Clean, unpatched `392bdd3` builds with the spike bridge. Includes detached-command and deletion tracking APIs; `write_file` still submits POST. |
| [Cleanup recheck with updated client](cleanup-updated-client.json) | Reused all five saved operation receipts with the unpatched Git client. All still reported `blocked`, with Boat reads returning 404. No new fixture or deletion request was needed. |

The fresh Git checkout at [`392bdd38d1ab3cf7d2fd62689b48cba525ea3e90`](https://github.com/nessalabs/boat-client-rs/blob/392bdd38d1ab3cf7d2fd62689b48cba525ea3e90/src/client.rs#L369) still uses `POST` for file writes. The successful transfer evidence therefore applies to the isolated corrected build, not that unpatched Git revision or a published release. The latest recheck rebuilt the bridge and read existing cleanup operations; it did not repeat the full workflow or create another fixture for the unchanged file-write request.

## What the probes establish

| Behavior | Observed result | Architectural consequence |
| --- | --- | --- |
| Create idempotency | Reusing a create key returned the same Boat. | Reuse provider support for provisioning; keep request identity stable. |
| Working directory | Default `/home/user`; both the tested relative and absolute task subdirectories worked. The old SDK's reported root matched this fixture. | Normalize the SDK contract and keep provider translation in the adapter; do not generalize this root to every backend. |
| Binary files | A 1,048,576-byte binary payload round-tripped unchanged using base64. | The native file API can carry checkpoint data. Larger limits, chunks, and performance still need tests. |
| Detached execution | A process survived replacing the Rust client and completed after seven seconds even though the submitted command timeout was one second. | Separate a command timeout from a managed-run deadline. Dropping a client must not imply canceling work. |
| Duplicate payload | A target-side exclusive file claim prevented the second submitted payload after the first launch receipt was deliberately discarded. | A small durable target-side launch claim is viable. Supervisor crash recovery and real network-loss injection remain untested. |
| Output retention | A 1 KiB tail omitted the beginning of a 700 KiB output and flagged truncation. | Tail polling cannot be advertised as complete event replay. |
| File round trip | Local baseline → Boat edits → local merge retained independent local edits, added/deleted files, binary contents, executable bits, and a symlink. | Git-based checkpoint transfer and reconciliation are suitable building blocks. |
| Conflicts | `git merge-tree` detected the conflicting edit without changing the destination file. | Stage and return a conflict result before applying changes. |
| Another machine | A second Boat restored the transferred result bundle and matched contents. | Client-mediated transfer works without target-to-target connectivity or a shared object store. |
| Worker handoff | Source stopped at counter 6; destination resumed saved state and reached 9; source state stayed unchanged. | Cooperative export/stop/import works. A real agent integration must supply its own safe checkpoint and resume hooks. |
| Cancellation | The owned synthetic process group stopped; the native result had no numeric exit code. | Do not require an exit code to represent canceled/signaled work; do require observed termination. |
| Hosting | Protected access worked with browser cookies; unauthenticated access was denied. | Return the full protected URL and document redirect/cookie behavior. A URL being returned alone is insufficient validation. |
| Archive/resume | A test binary file survived stopping, reaching `archived`, resuming, and readiness. | File persistence is supported by this probe. Live memory/process continuation was not tested. |
| Deletion | Accepted operations progressed through `processing` to `blocked`, including the fixture explicitly archived before deletion. No blocker detail was exposed in the inspected operation response. | Preserve an operation reference and pending/blocked status. A missing Boat is not proof of completed deletion. |

## Cleanup status

Five short-lived fixtures were created across the failed, corrected, and focused runs, each with a 900-second TTL and synthetic data. Deletion was accepted for all five. The focused hosting fixture was explicitly confirmed archived before deletion. All five are absent from ordinary Boat reads, but their background operations still report `blocked`; storage purge is not confirmed.

Operation IDs and exact fixture IDs are retained in [cleanup verification](cleanup-verification.json) and the [updated-client recheck](cleanup-updated-client.json). The historical verification was restricted to these recorded fixtures and reused their saved receipts. No account-wide cleanup or deletion of unrelated resources was attempted. Resolving the provider's blocked deletion state remains a release gate for promising completed destruction.

## Limits of this evidence

This validates primitives and a small combined workflow, not the entire proposed SDK. The fixture used an ordinary Git repository and an ignored synthetic `.env`; it does not prove the proposed general source filter, tracked-secret exclusions, non-Git snapshots, or crash-safe apply. It did not test concurrent editor writes, a killed supervisor with orphaned children, cross-vendor transfer, a local container, an arbitrary SSH host, native LLM-agent resume, or cross-provider VM-memory migration.

The later environment/authentication requirements are also untested: detecting/installing real agent runtimes, native Boat agent credentials, subscription sign-in, copying or refreshing an auth cache, and GPU/resource preflight. The fixtures used `noEnv=true`; no real agent credentials were transferred. These are separate next probes in the [plan](../plan.md).

Local validation also passed `cargo test --offline` (the existing local execution test) and compilation of the Rust bridge. The frozen patched build's provenance is separate from later changes in the shared dependency.

## Historical conclusions

The 0.2 implementation now includes the pinned file-write correction, public backend contract, managed supervisor, checkpoint/sync workflow and explicit deletion results. Real SSH/container conformance and native agent authentication remain follow-up validation in the [plan](../plan.md).

## Public source verification

The vendored client is now Boat client commit `4d68256bb0b6a889633abc567fb09214681874db`
from https://github.com/nessalabs/boat-client-rs. Its existing 59-operation surface
was checked against published `@boatdev/sdk@1.9.7`; the client passed independent
quality/adversarial review, local mechanical/performance checks and hosted CI.
Those are offline/local checks, not a repeat of the historical live Boat probes.

Before public source publication, live resource identifiers and local account
paths in these JSON reports were replaced with private aliases. Original reports
are retained locally outside the publication tree. Outcome counts and measured
results are preserved; `publication-redactions.json` inventories modified reports.
Published report labels and paths are normalized. The archived build digests
describe historical source contents, not the current tree. Public aliases cannot be used to reconnect or inspect
cleanup; no completed cleanup is inferred from redaction.

Publication checks passed: 112 all-feature tests, 103
no-default-feature tests, three doctests per phase, formatting, strict Clippy,
warning-free Rustdoc, native supervisor build and the real managed-run
performance qualification. Five independent provider configuration and isolation
contracts passed after their pre-change failures were retained. Independent
quality, adversarial and publication-security reviews are under `docs/reviews`.
