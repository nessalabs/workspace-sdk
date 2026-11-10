# SPACE attachment adversarial acceptance

**FINAL PASS** for source `068d8193438ba14e2d44ce915ac1f1e95c4bb377`. No unresolved P1/P2 findings affect this scope. Reviewer: separate adversarial subagent `workspace_handoff_adversarial_review`; the provider implementation and SPACE contract tests were authored by other agents. This reviewer independently researched the pinned upstream protocol, challenged source and fixtures, and verified the final evidence. This decision does not qualify a live SPACE installation.

## Scope and authority

Reviewed the optional `space` feature, six provider modules, public exports/configuration, documentation, mechanical gate selection, HTTP/SSE/native fixtures, and the follow-up test-only GPU byte-boundary fixture. The final 15-file manifest matches every reviewed file's byte length and SHA256. Its raw SHA256 is `936884e528dfb93a198fe5784526b36d19ada4944382e557cc60b73280b6c70e`. GPU production source remains unchanged at SHA256 `145915bb5d3dd86479eabae9a1a79ef265c48409006b25f4ee3b8b5416ddbe5b`.

SPACE supplies an existing sandbox and short supervisor RPC transport. Existing SDK managed claims, native run references, retained output and cancellation remain authoritative. Reconnect validates the entire configured locator and probes existing state without initialization. There is no new session-to-run journal or replacement permission derived from a disconnected session.

The wire comparison used actual DTO, native SSE producer/protocol and route source at upstream commit `3f0ed159f6249fc9029f6f44d7f7db7d954d1f14`, rather than treating an older OpenAPI baseline as complete. Source URLs and hashes are recorded in the [qualification evidence](../evidence/space-provider-2026-10-18/qualification.json).

## Findings and dispositions

All following P2 findings or acceptance gaps are closed in the reviewed source:

- **Pinned wire mismatch:** `token_transport` is an array, and normal successful SSE emits matching `process_started`, shared stdout/stderr sequences starting at 1, `process_exited`, then `closed/backend_closed`. Early shorthand from this reviewer contributed to the scalar fixture mismatch; direct upstream source comparison corrected it. The parser now accepts the real array/trailer, requires start before output/exit, validates identities and order, and rejects missing start, gaps, closed-only success and post-exit output. Matching exit plus complete EOF is also explicitly supported; closed alone never proves success.
- **Control namespace:** an arbitrary sibling name could evade mandatory checkpoint exclusion. Control state now uses the reserved parent `.workspace-sdk/{id}` namespace, rejects roots entering that namespace and binds all endpoint/sandbox/user/executable/root/control configuration into reconnect validation. Trusted parent and same-user limitations are explicit.
- **Ambiguous private upload:** deleting a prepared slot while a late create-capable append could still arrive risks recreation with different permissions. Unknown upload outcomes now retain the prepared 0600 slot, never invoke partial input and never replay append. Confirmed completion alone permits scoped cleanup under the original deadline. Explicit same-user configuration preserves the native owner/mode checks.
- **Implicit replay and bounds:** the HTTP client explicitly disables retries, redirects and inherited proxies. One deadline covers the entire operation, including private preparation/upload/invocation. JSON, SSE wire/frame/output and request bounds are enforced. Expanded framing accommodates a full native 256-KiB file-read response after JSON/base64 expansion; the full-chunk test exercises that boundary.
- **Fixture ownership:** fixtures originally risked deleting native state before HTTP handlers finished, and error paths could detach handlers or panic again during unwinding. Every native case now declares its retained directory before the HTTP fixture so handlers join first. Listener/error paths drain owned handlers; socket reads/writes use remaining deadlines. Native cleanup requires validated owner, terminal state and active-lock quiescence; uncertainty or unwinding retains evidence. The eight-way same-claim start case checks one real append and a shared native identity.
- **GPU byte-boundary fixture:** an existing pipe-backed exact-limit test returned no observation in the first Ubuntu gate. The precise incident cause remains unproven. A separate inherited-writer reproduction establishes a possible mechanism, not that incident's diagnosis. The revised test obtains real child output in a private finite file, confirms successful reap and exact 65,536/65,537-byte lengths, then rewinds and passes an owned descriptor to the unchanged collector. ECHILD does not trigger a signal to an identity whose ownership is unknown. Separate pipe/socket, held-writer, backpressure and timeout tests remain unchanged.

## Actual verification

[Full gate 39144874948](https://github.com/nessalabs/workspace-sdk/actions/runs/39144874948) completed successfully on Ubuntu 24.04 and macOS 15 at the exact source SHA above. Independently checked raw run metadata, both completed jobs and raw log SHA256 `1a93fa58f5385c38b21f4c2611944e9074c99cc908c930cb1a7ab32e9ccb7750`.

The logs contain all 12 SPACE cases under all-features and again under SPACE without Boat on each OS: 48 executions. No-default-features correctly executes zero optional SPACE cases. The full script also passed fmt, all-target/all-feature Clippy with denied warnings, builds/examples, retained tests, docs and performance checks. The known no-default `requires_private_transport` dead-code warning remains visible; this is not a zero-warning claim.

Independently decoded all four performance objects from the raw log and compared their complete objects with the saved evidence: three managed and fifteen PTY samples per OS, debug Linux x86_64 and macOS aarch64. These are hosted qualification samples, not a statistical baseline or SPACE transport benchmark.

Three actual parent-run semantic mutations failed the intended assertions: removing the started guard accepted `MissingStarted`; cleaning up unknown upload removed its prepared slot (`NotFound`); relaxing the GPU cap accepted over-limit output. Probe log hashes match evidence, exact source restoration was checked, and restored local checks plus the final full cloud gate passed. These probes establish sensitivity to those particular faults, not exhaustive fault coverage.

Failures remain recorded: the first local contract run passed three cases before a handler WouldBlock and a second Drop panic caused SIGABRT; its precise socket failure cause was not established. Cloud run `39144143639` at the earlier candidate passed macOS and failed Ubuntu at the existing GPU exact-byte case. Its metadata/raw hash match retained evidence. Neither failure is erased or represented as a successful qualification.

## Limits

Acceptance covers isolated HTTP/SSE simulation exercising actual local native supervisor execution. No live SPACE, KVM/VM isolation, remote network durability, billing, provisioning, idle/suspend/delete policy or detached-supervisor survival in an actual deployment was exercised. Lifecycle mutations and port publication are unsupported; the sandbox owner manages cost and cleanup.

The file API is explicitly incubating at the pinned revision. Same-user untrusted processes are not isolated by this adapter. Private payloads can remain after uncertain delivery and require owner cleanup after transport quiescence; SPACE may retain uploaded bytes and workload output may expose secrets. No indefinite provider idempotency, arbitrary external-effect exactly-once execution, automatic failover, physical power-loss durability or deployment claim follows from these tests.
