# SPACE provider independent quality review

**FINAL PASS — no unresolved P1/P2 findings in the reviewed scope.**

Reviewer: independent quality reviewer `nessa_session_contract`. I authored no
production implementation or acceptance tests for this increment. I reviewed
Clean Code, SOLID, dependency direction, cohesion, ownership, bounded work and
the relationship between claims and actual checks. The separate adversarial
review is required independently of this decision.

## Exact scope

Base: `849984e4a6e81cb36469a03dfe262a0be3efa0b5`.
Final source: `068d8193438ba14e2d44ce915ac1f1e95c4bb377`.
The [source manifest](../evidence/space-provider-2026-10-18/source-manifest.json)
identifies all 15 reviewed files. Its SHA-256 is
`936884e528dfb93a198fe5784526b36d19ada4944382e557cc60b73280b6c70e`;
I independently checked every listed file against its recorded hash after the
final cloud run.

Inspected all six `src/providers/space` modules, provider exports, Cargo feature
and dependency wiring, README and provider documentation, quality-gate feature
selection, all SPACE contract tests and their HTTP/native fixture, and the
test-only GPU boundary change. I also traced the existing `ManagedBackend`,
`ITarget`, supervisor protocol classification and native run-state terminal
predicate used by the adapter and fixture. Production GPU collection remains
unchanged, SHA-256
`145915bb5d3dd86479eabae9a1a79ef265c48409006b25f4ee3b8b5416ddbe5b`.

## Design and ownership assessment

The provider implements existing SDK interfaces and reuses native supervisor
claims, output storage and cancellation. It does not introduce a second run
journal or agent/provider-specific recovery authority. Configuration, locator
binding, HTTP transport, SSE framing and native request transport have focused
owners. The adapter is optional and attaches to an existing sandbox; lifecycle
mutation remains explicitly unsupported through existing attached ownership.

Configuration supplies credentials and endpoints explicitly. Debug and durable
references omit tokens. HTTP redirects, inherited proxies and automatic retries
are disabled. Attach credentials are sent only after the configured data-plane
origin, exact session path and receipt identities have been checked. The pinned
upstream DTO and native protocol were inspected separately from the simulator;
the final receipt uses the actual token-transport array, process-start and
process-exit events. Simulation alone is not external compatibility proof.

One deadline spans a private operation's preparation, upload, invocation and
remaining cleanup. JSON, request, wire, frame and decoded output bounds are
explicit. The existing exhaustive supervisor operation classifier decides which
inputs require private transport. Ambiguous upload keeps its prepared private
slot and does not invoke or replay potentially partial input. Successful native
execution and uncertain HTTP delivery remain separate facts. Reconnection
validates the existing locator and native identity rather than initializing
replacement state. The full 256-KiB native file chunk has a real round-trip
oracle.

The fixture executes the actual installed supervisor. Its append-once,
retained-output, cancellation and missing-identity checks observe real native
effects. Eight concurrent starts share one native run identity; that establishes
the tested launch-claim property, not exactly-once arbitrary external effects.

## Findings, corrections and re-review

All findings below were P2 and are closed in the final source:

1. **Unbounded fixture subprocess/output ownership.** Initial `Command::output`
   and unconditional joining could hang cleanup. The fixture now owns the child,
   polls a five-second deadline and sampled output lengths, kills/reaps on
   failure, and performs bounded reads from private output files. The sampled
   byte check is not an operating-system disk quota.
2. **Deadline regression did not exercise continued frames.** An initial sleep,
   and then partial-frame dripping, could pass an implementation that reset its
   deadline. The final fixture sends complete heartbeat comments every 10 ms
   across 250 ms; the 50-ms RPC must still expire with exactly one command.
3. **Native cleanup inventory lacked transport quiescence.** Temporary directory
   destruction could precede a late accepted handler. Every native directory is
   now constructed before its fixture, so transport handlers join first. The
   bounded inventory then verifies ownership receipts, terminal state and the
   released native activity lock. Lost/unknown ownership retains evidence;
   unwinding unconditionally preserves the directory.
4. **Listener failure detached existing handlers.** Budget/setup/accept failure
   could panic through a vector of thread handles. Those paths now record the
   failure, reject or stop admission and join all already-owned handlers.
5. **Body and response socket waits could restart their budgets.** A single
   timeout around `read_exact` and repeated `write_all` calls did not establish a
   whole-operation deadline. Manual read/write loops now recompute the remaining
   deadline before each OS call, including ordinary, redirect and drip replies.
6. **GPU fixture cleanup after ECHILD lacked ownership.** The new finite-output
   helper now fails without kill/wait when `try_wait` reports ECHILD. Other
   owned-child failure paths retain cleanup.

I re-read these corrections, their adjacent cleanup paths and the final frozen
files. No remaining P1/P2 findings were identified.

## Actual verification

I inspected raw logs and metadata rather than treating reported summaries as
passing checks. [Qualification](../evidence/space-provider-2026-10-18/qualification.json)
records the detailed commands, hashes and retained outcomes.

- Final [GitHub Actions run 39144874948](https://github.com/nessalabs/workspace-sdk/actions/runs/39144874948)
  passed `scripts/quality-gate.sh` on Ubuntu 24.04 and macOS 15 for the exact
  final source commit. This includes formatting, all-target/all-feature Clippy
  with warnings denied, builds/docs, all-feature and no-default-feature tests,
  the SPACE-only selection and managed/PTY performance checks.
- All 12 SPACE tests passed with all features and with SPACE without Boat on
  each OS: 48 SPACE test executions. The no-default-feature selection excludes
  this optional provider. Logs also show the GPU boundary case passing on both
  platforms.
- The final raw cloud log SHA-256 is
  `1a93fa58f5385c38b21f4c2611944e9074c99cc908c930cb1a7ab32e9ccb7750`;
  I independently checked this hash and the successful job metadata.
- Three managed and fifteen PTY samples per OS are present. These hosted debug
  samples qualify the exercised environment; they are not a statistical
  performance regression baseline or live SPACE performance measurements.
- Three temporary semantic faults actually failed runtime assertions: accepting
  missing `process_started`, removing a prepared slot after unknown upload, and
  accepting 65,537 GPU bytes. Their logs contain the expected semantic failures,
  not compilation failures. Source hashes were restored before final checks.
- Restored local checks passed 130 library tests and 12 SPACE cases, alongside
  formatting and the earlier focused SPACE Clippy/checks. I did not run an
  additional competing build or mutation during the frozen gate.

The first local fixture run aborted after a header error and a second panic
during Drop; it is retained, not counted as a pass. The exact original cause
was not established. The earlier cloud run `39144143639` passed macOS but
failed Ubuntu at the existing exact GPU byte-boundary observation. That
incident's return reason remains **unproven**. The new boundary fixture verifies
successful actual child output and exact length in a finite private file,
rewinds it, and passes it to the unchanged collector. Separate pipe/socket,
held-writer, backpressure and timeout tests remain intact. A separate POSIX
inherited-writer demonstration establishes a possible mechanism, not the cause
of that CI incident.

## Acceptance limits

This pass covers the pinned attach-only SDK adapter and isolated HTTP/SSE tests
with real local native execution. It does not qualify a live SPACE deployment,
Linux/KVM isolation, cross-host network durability, provisioning, billing,
physical host loss or agent-conversation migration. The supervisor must already
be installed, and guest user/parent directory trust remains an application
precondition. The pinned `files:write` route is incubating; private uploads may
be retained by SPACE, and workload output can disclose secrets. Unknown uploads
require owner cleanup after transport quiescence. No Nessa core source change,
private controller publication or new billable resource is part of this scope.

**Final decision: PASS for this exact source and declared evidence scope.**
