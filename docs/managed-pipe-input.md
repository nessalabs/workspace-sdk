# Explicit managed stdin and EOF

`Workspace::start_piped(LaunchRequest)` starts or reconnects to a managed run
whose stdin stays open. It uses the existing RunRef, detached supervisor and
launch claim. Ordinary `start_with` and terminal launches retain their existing
fingerprint encoding and input behavior. Reusing an ID with a different mode
conflicts. Piped launches reject both ordinary and private launch-time stdin
before creating a claim; input is submitted separately.

`Run::write_input(&SecretInput)` submits one nonempty request of at most 65536
bytes. A native owner performs one nonblocking write without an input queue:

- `Accepted { bytes }` identifies the exact prefix admitted to the kernel pipe,
  including zero when the pipe is full. It does not prove child consumption.
- Only a known unaccepted suffix may be submitted again. Callers must serialize
  an entire logical frame themselves; concurrent writers' suffixes can interleave.
- `Unknown` means acknowledgement was lost or malformed and delivery may have
  occurred. There is no automatic retry, reconnect-and-replay or rollback.
- A definitive refusal is an error, including empty/oversized input, a wrong
  workspace or a non-piped claim. A live owner refuses input after EOF.
  During control teardown a fresh submission can conservatively return
  `Unknown` or an unavailable-owner error even after an earlier `Closed` ACK;
  this never permits another byte to enter the closed endpoint or be replayed.

`Run::close_input()` sends real EOF by dropping the owner's stdin writer. It is
serialized with input in the same owner loop. Await write acknowledgements before
closing when ordering matters. `Closed` means the endpoint is closed, not that
accepted input was consumed or execution succeeded. Repeated close is idempotent;
an unknown close can be retried on the same RunRef without replaying input.
A confirmed terminal piped run also has closed input. Lost or unknown ownership
never establishes `Closed`.

Run handle drop still does not cancel execution or close input. Aborting an input
future cannot revoke dispatched bytes. Use explicit EOF and confirmed terminal
state, or exact-run cancellation, before releasing execution ownership. Input
controls have bounded frames and waits and service at most one client per native
owner iteration, allowing output, cancellation and deadlines to keep progressing.

Transient input uses private provider transport and is excluded from launch
claims and debug output. A child may deliberately echo it into retained output;
this API does not sanitize child output. Stdout and stderr remain separate raw
byte logs through `output_bytes`, with explicit retention-gap semantics and no
producer backpressure guarantee.

Unsupported providers reject the optional pipe operations. An unavailable or
old supervisor cannot silently substitute PTY behavior. This does not add child
environment isolation, an agent host adapter, or conversation migration.
