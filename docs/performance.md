# Local runtime performance qualification

Run this smoke qualification at each implementation gate alongside correctness,
architecture review, and adversarial review:

```sh
cargo build --bin workspace-supervisor
cargo test --test runtime_performance -- --ignored --nocapture
cargo test --test runtime_pty_performance -- --ignored --nocapture
```

The test is ignored in ordinary `cargo test` runs so the gate must invoke it
explicitly. It uses the real local provider and native Rust supervisor, with an
optimized Rust producer compiled before measurements. It requires Unix and a
working `rustc`; it creates no cloud resources and uses no credentials. Workspaces
and the producer are temporary. The producer intends to exit after 20 seconds,
but a blocked write could delay that exit. The supervisor enforces a 25-second
command timeout, and each sample has a 30-second outer watchdog.

The harness runs three sequential samples. Each starts a producer writing 4 KiB
to each output stream approximately every millisecond, waits for output, drops
the client, reconnects, retries the same launch, and cancels while output is
active. Before cancellation, each stream must have advanced beyond 128 KiB with
a combined 32 KiB retention budget. This measures a modest concurrent I/O load,
not maximum throughput.

Pass/fail assertions cover the observable contracts:

- Reconnection and launch retry execute the producer exactly once.
- Old cursors expose retention gaps, and combined stdout/stderr pages remain
  within the requested byte limit.
- Cancellation returns and persists `Canceled` while the producer is active.
- Terminal draining recovers exactly each stream's retention budget with the
  expected ASCII content, advances cursors without new gaps or offset loss, and
  eventually returns stable empty completed output.

The JSON report is written only after all samples pass. Its default location is
`target/runtime-performance.json`, which Git ignores. An existing report at that
path is removed before the test runs, so a failed invocation cannot masquerade as
a fresh success. Override the destination with `WORKSPACE_SDK_PERF_REPORT`; its
parent directory must already exist. Avoid sharing one report path between
concurrent invocations.

The report records raw milliseconds for launch acknowledgement, launch to first
observed output, provider reconnect, idempotent launch retry, one output read
under load, and cancellation. Reconnect timing covers construction of the
reconnected backend; the following retry verifies interaction with the persisted
run. First-output timing includes polling (10 ms intervals), supervisor startup,
OS scheduling, and output buffering. It also records retained bytes, absolute
stream offsets, platform, architecture, build mode, and timestamp.

These latency samples are **not an established regression baseline**. There are
no hardware-dependent speed thresholds; watchdogs detect hangs rather than
speed regressions. Three samples are insufficient for reliable tail percentiles.
For comparisons, retain JSON artifacts with the Git revision, `rustc --version`,
OS version, hardware, and competing workload recorded by the gate runner. Compare
like-for-like builds and investigate repeated changes before adopting budgets.
The pipe test does not qualify cloud/SSH/container performance, PTY behavior,
native terminal rendering, process RSS, CPU consumption, or descendant cancellation.

## PTY controls under concurrent output

`runtime_pty_performance` separately qualifies the public runtime-host adapter,
local provider, supervisor, and raw `pty-runtime` dependency. It runs three
independent populations of one session and three of four sessions, each in its
own temporary workspace. The optimized standalone producer is compiled before
measurement. Every session continuously emits numbered 1 KiB ASCII records,
approximately once per millisecond. All sessions are started before concurrent
input, resize, and cancellation tasks run; the active population decreases as
cancellation completes. This is a modest output workload, not peak throughput.

Each producer must advance beyond 128 KiB before control measurement. A child
file acknowledgement confirms that input reached the workload; a second confirms
that `stty size` observed the requested 120-column, 40-row dimensions. Resize
timing includes a subsequent input request and the child size query. After
cancellation, every retained output byte is checked against the numbered record
at its absolute stream offset. Replay must stay within the 128 KiB bound and
advance contiguously after any initial retention gap. Output must also advance
during controls. The shared fixture enforces a 40-second case watchdog and always
attempts cancellation and workspace destruction, including on panic or timeout.

`target/runtime-pty-performance.json` is cleared before each run and written only
after all 15 sessions and their cleanup pass. It records raw launch/readiness,
observed input, observed resize, and cancellation milliseconds, absolute output
bytes, retained bytes, and observed average bytes/second from launch through
cancellation. Startup, polling, resource probes, and control overhead contribute
to that average. Readiness is a child-file acknowledgement, not first-output
latency. Timing samples have no hardware-specific acceptance threshold.

The report includes one live supervisor RSS sample (`ps`, KiB) and numeric open
descriptor count (`/proc/<pid>/fd` on Linux; `lsof` on macOS) for each session.
These snapshots measure the SDK supervisor process only, excluding its native
guardian and workload descendants. They are neither peak memory measurements
nor leak proofs. Probes themselves perturb concurrent timing. Platform, CPU
architecture, build mode, and timestamp are recorded; preserve the report with
the gate's Git revision and environment. This qualification requires `rustc`,
`ps`, `/bin/stty`, and, on macOS, `lsof`. Remote providers, terminal rendering,
long-duration resource growth, CPU cost, and maximum saturation remain outside
this local smoke qualification.
