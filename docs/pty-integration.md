# Native authentication integration

The reviewed PTY public API is available in the merged pty-runtime repository. Workspace SDK has
the provider-neutral authentication controls and failure contracts needed for
its adapter; it does not yet claim a working PTY or native Claude login UI.

## Current public controls

After `prepare` returns a required authentication action, call
`start_authentication` and retain its `RuntimeProcessRef`. Subsequent controls
take the identical `PreparationRequest` and that exact reference:

- `authentication_process_state` observes the current saved attempt.
- `read_authentication_output` returns bounded text and independent output cursors.
- `write_authentication_input` delivers transient `SecretInput` bytes when supported.
- `resize_authentication` accepts a validated `RuntimeTerminalSize` when supported.
- `cancel_authentication` requests cancellation without discarding the receipt.

The managed backend currently implements device-login output, observation, and
cancellation. Terminal input/resize return `Unsupported` until a real adapter
exists. Terminal emulation snapshots and raw byte replay belong to the reusable
PTY layer; authentication text is not a substitute for those interfaces.

Process references contain an exact host scope as well as a process ID. The
backend derives its scope from the full workspace reference, so an equal run ID
on another workspace cannot be rebound through the current host. Preparation
receipts also bind the complete request to this scope. Legacy unscoped process
references and receipts are rejected; reconcile their original process state
before migrating them rather than assuming they identify the current host.

## Concurrency, bounds, and uncertain responses

Every service operation takes a nonblocking per-preparation store lease. Memory
stores coordinate service instances sharing that store; file stores coordinate
instances sharing the same directory through file locks. Busy operations return
`Conflict` promptly instead of waiting behind an installer. Different preparation
IDs remain independent. All controllers of one preparation must use the same
coordinated store. Direct store users must acquire leases around their complete
read/side-effect/save sequence; atomic saves alone do not serialize workflows.

The login launch key includes the complete request fingerprint, host scope and
attempt counter. A failed receipt acknowledgement retries that same key. Host
implementations must reconnect to the same launch and reject rebound intent.
This is not a guarantee that arbitrary interrupted installer/setup commands are
automatically safe to repeat.

Input is bounded to 64 KiB per submission; output to 2..=128 KiB of combined raw bytes per page. Cursors count raw bytes.
Lossy UTF-8 decoding can replace characters split across pages and expand the
returned text to at most 384 KiB.
Each control host call has a five-second budget. Input and resize first inspect
process state, so those methods can spend two host-call budgets. Storage adapters
must provide promptly failing lease acquisition; native filesystem stalls are
not converted into hard real-time guarantees.

An accepted input prefix is not proof the child consumed it. Partial acceptance
permits submitting only the remaining suffix. An acknowledgement timeout returns
`Unknown`; the service never automatically replays the input. A cancellation
error retains the current attempt. Inspect that same process to reconcile it;
only confirmed exit permits a subsequent attempt. If the host reports `Lost`,
restore/reconnect the original host's evidence and inspect again. There is no
unsafe reset that fabricates termination.

Input is absent from receipts and ordinary Debug output. A terminal may echo it,
and a child may print it; output is returned to the caller and is not sanitized.
Native authentication status, not process exit zero, determines login readiness.

## Remaining adapter work

Use the PTY package's reviewed facade for spawning, ordered input, resize,
attachment/replay, cancellation and ownership. Preserve project versus private
control-directory execution and environment removal. Remote supervision must
retain a PTY owner independently of individual request processes; invoking a
short-lived command per operation cannot own a lasting interactive session.
Qualify that lifecycle with synthetic terminal prompts, cross-target references,
detach/reattach, interrupted transport, saturated input/output and cleanup.

The local managed-run performance smoke does not qualify PTY throughput, native
terminal parsing, RSS or remote behavior. The PTY integration must add those
checks before claiming them. Every acceptance gate also requires the independent
design and adversarial reviews in `coding_standards.md`.
