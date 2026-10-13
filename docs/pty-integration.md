# Native authentication integration

Workspace SDK integrates the merged `pty-runtime` public API at revision
`c68837e2b7bf1c9009e1a4587a1a58107ec0425c`, with rendering features disabled.
A dedicated target-side supervisor owns each raw PTY session independently of
SDK handles and individual control requests. Synthetic local fixtures qualify
terminal behavior; real agent sign-in and live remote PTYs remain unvalidated.

## Current public controls

After `prepare` returns a required authentication action, call
`start_authentication` and retain its `RuntimeProcessRef`. Subsequent controls
take the identical `PreparationRequest` and that exact reference:

- `authentication_process_state` observes the current saved attempt.
- `read_authentication_output` returns bounded text and independent output cursors.
- `write_authentication_input` delivers transient `SecretInput` bytes when supported.
- `resize_authentication` accepts a validated `RuntimeTerminalSize` when supported.
- `cancel_authentication` requests cancellation without discarding the receipt.

The managed backend supports terminal launch, transient input, resize, output,
observation and cancellation. Terminal launches start at 80 columns by 24 rows
and retain at most 128 KiB. Pipe-based device login remains supported; input and
resize on those runs return `Unsupported`. Terminal emulation snapshots are not
part of this adapter. Authentication text is a lossy view of raw replay bytes.

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

## Ownership and verified behavior

The supervisor stages the native runtime before starting asynchronous workers.
A durable launch claim binds execution intent, retention, terminal mode and
initial dimensions. Reconnecting after dropping SDK handles attaches to the
same run; changing intent under that key conflicts. An owner that disappears
becomes `Lost`; its claim remains retained and cannot silently launch replacement
work. Deletion stays blocked while termination is uncertain. A missing bare
executable discovered before native launch instead retains an exit-127 receipt
and a bounded diagnostic, permitting safe lifecycle operations.

Terminal stdout and stderr share one PTY stream, exposed through the stdout
cursor; stderr is empty. Absolute byte positions survive retention loss. A gap
also records incomplete native draining, including descendants holding the
terminal after leader exit. `complete` means no more retained bytes can arrive;
it does not promise gap-free output. Shutdown releases owned descendants before
publishing completion. Normal exit, requested cancellation and deadline expiry
remain distinct outcomes.

Control sockets are private to the run. Connection, request writes and incoming
frames each have a 250 ms total budget; trickling a frame does not extend it.
Native input and resize waits are bounded to 500 ms, and replies have a separate
two-second budget. Once input may have been dispatched, missing or malformed
acknowledgement becomes `Unknown` without automatic replay. A typed refusal
from the supervisor before dispatch preserves definite errors such as a missing
run; transport errors never establish that refusal by their error variant alone. Remote transport
uses protected request files or private standard input rather than command
history. An uncertain remote cleanup can leave a protected transient request
file; this is not a promise of secure deletion.

The acceptance and adversarial fixtures cover real terminal/foreground checks,
project and private working directories, effective-user executable lookup,
environment removal, input/resize, exact replay tails, reconnect and duplicate
prevention, wrong-workspace references, lost or malformed acknowledgements,
slow frames, cancellation, incomplete drains and owner death. Provider HTTP
contract tests verify private launch/input delivery and failure cleanup; they do
not certify a live remote supervisor. Deploy the matching supervisor binary on
remote targets before using terminal controls.

The gate retains the existing managed-run performance qualification and adds
three iterations each at one and four simultaneous PTYs. It checks exact
retained bytes, observed input/resize and cancellation under output, and records
owner RSS/descriptor snapshots and observed throughput. These samples are not a
statistical regression baseline, peak-memory measurement or long-running leak
test. See [performance evidence](performance.md) and the dated independent
reviews under `docs/reviews` for measured results and acceptance decisions.
