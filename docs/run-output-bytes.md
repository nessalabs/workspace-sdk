# Retained output as bytes

`Run::output_bytes(cursor, limit)` reads the original retained stdout and stderr
bytes through `OutputBytesPage`. It uses the same workspace-qualified `RunRef`,
absolute byte cursors, retention, and lifecycle observations as `Run::output`.
There is no hidden read position: repeat the same cursor to reread the same
retained range, or persist `next` and reconnect to continue reading.

The combined page limit is 2 through 262144 bytes. Stdout receives half the
budget rounded down; stderr receives the remainder. A two-byte limit can split
a UTF-8 codepoint across pages. Concatenate the bytes or use an incremental
decoder when interpreting text. Invalid UTF-8 and NUL bytes remain unchanged.
The existing `output` method remains a lossy text view of each byte page.

`gap` reports missing retained bytes or a supervisor output-loss marker. A
protocol consumer must fail that attachment rather than silently skip or replay
missing frames. `complete` requires a confirmed terminal process state and both
streams drained. It does not attest external writer quiescence or remote billing
cleanup. Output buffers remain bounded logs rather than backpressured pipes.

Built-in managed supervisor transports encode bytes as base64 at the wire
boundary. Providers without the optional capability return `Unsupported`; an old
supervisor may reject the operation. No client falls back to text decoding.
Neither debug representation includes output content; serialized wire responses
contain the output and must be treated accordingly.

This adds retained byte reads only. It does not provide streaming stdin, input
EOF, a cleared child environment, a duplex agent transport, or agent conversation
migration. Those require separate contracts and qualification.
