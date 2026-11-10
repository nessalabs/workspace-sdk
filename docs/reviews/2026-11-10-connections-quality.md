# Connections (W4): independent quality review

Reviewer: a separate code-quality agent. Scope:
- `src/ports/i_connection.rs`
- `IBackend::connection` and its `ManagedBackend`/`ITarget` delegation
- `src/providers/sandbox/connection.rs` and its tests

The Boat connection was ported from the same owner's application (`ssh.rs`). That application's own parts stayed there: the MCP relay mapping and the sign-in script.

Three rounds were reviewed: 5441bd5, ac9897f and b417211. The final fixes are in the commit that follows them.

## Decision

PASS with the final fixes. No P1 or P2 is open.

## Findings and dispositions

| Round | Sev | Finding | Disposition |
| --- | --- | --- | --- |
| 1 | P1 | `run` dropped the exit status | Fixed. `RunOutput { end, output, stderr }`. ssh's exit 255 is `Lost`. |
| 1 | P1 | A stream or file channel could not tell how it ended (`dyn Any`) | Fixed. `IConnectionProcess::wait -> ProcessExit`. |
| 1 | P2 | The docs promised cleanup the code did not deliver: keys are never revoked | Documented on `SandboxProvider` and `IBackend::connection`. |
| 1 | P2 | Drop relied on field order | Fixed. An explicit `Drop` kills, then reaps on a task. |
| 1 | P2 | The port's shape follows OpenSSH; `run` and `exec` are unclear | The docs now say when to use `run` and when `exec`, and that the files channel is SFTP v3. `File` input is kept on purpose. |
| 1 | P2 | Public structs cannot grow | `SocketForward` is private with a checking `new`. The plain data types stay open so other crates can implement the port; adding a field to them is a breaking change. |
| 1 | P2 | No timeout or cancellation | Fixed. `run` takes a timeout, and dropping ends the command's session on the machine. |
| 1 | P2 | Test gaps | Added tests that run without sshd: host key, forward path, refused authorizations, and the wrapper under a real `setsid` and an sshd-named parent. Added ignored end-to-end tests with sshd for every behaviour. |
| 1 | P3 | Quoting is duplicated; naming; `connect` is long; the forwarded socket is left behind | Fixed. `managed::quote`, renamed items, `connect` split into steps, and a private directory for the forward that is removed. |
| 2 | P2 | A lost run was an error, and a command's own 255 is ambiguous | Fixed. A lost run is `Ok(Lost)` and keeps its output. `Err` only when the command could not start. The ambiguity is documented. |
| 2 | P2 | The diagnostic carried the command's own stderr | Fixed. It is a separate, bounded `stderr` field and never part of an `Error`. |
| 2 | P2/P3 | The ending guarantee was wider than the wrapper delivered | Fixed. The whole session gets TERM, then KILL after 5 s. The wrapper refuses to run without `/proc`, when it does not lead its session, or when its parent is not sshd. |
| 2 | P2/P3 | Drop blocked a runtime worker | Fixed. |
| 3 | P1 | The wrapper's unit tests need Linux but run on the macOS CI leg | Fixed. `#[cfg(target_os = "linux")]`. |
| 3 | P3 | A machine that can never bind a command reads as "may have run" | Fixed. `connect` runs `true` through the wrapper first and fails if it does not succeed. |
| 3 | P3 | stderr was flattened to one line; the name was unclear | Fixed. Lossy UTF-8 like `output`, and renamed `stderr`. |
| 3 | P3 | `Debug` prints command output | Accepted. It is documented on the field, as for a command's output. |
| 3 | P3 | The grace period needs a `sleep` that takes fractions | Documented. |
| 3 | P3 | One unit test always waits the 5 s grace | Accepted. It is what that test proves. |

## Left open, accepted

- Two things are not tested: the `ManagedBackend::connection` delegation, and the `ITarget` default of `Unsupported`.
- There is no shared helper for the hardened SSH options used by this module and `providers/ssh`.
- There is no `Workspace` facade method; callers use `backend()`.
- There is no architecture.md entry yet. It will be written when the application moves onto this port.
- Each long stream wakes the machine once a second: the watcher's `sleep 1`.
