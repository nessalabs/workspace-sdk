# Connections (W4): adversarial review

Reviewer: a separate adversarial agent. It worked against a real local OpenSSH
9.6 sshd (dash as `sh`, bash as the login shell) and probed the wrapper's own
text. It looked for races, leaks, secret exposure, injection, unbounded waits
and misleading tests. The scope and the three rounds match
`2026-11-10-connections-quality.md`.

## Decision

PASS with the final fixes. No P1 or P2 is open.

## Findings and dispositions

| Round | Sev | Finding | Disposition |
| --- | --- | --- | --- |
| 1 | P1 | `-O check` passes before a `-R` forward is confirmed. A failed forward later killed the master, and sessions then quietly signed in again on their own | Fixed. The master starts without forwards. The forward is asked for with `-O forward` and awaited. Sessions through the master use `ProxyCommand=false`. Commands are refused once the master has ended. |
| 1 | P2 | Dropping a command left it running on the machine (sshd sends nothing) | Fixed. Each stream or run has its own connection and runs under a wrapper. When sshd's process for the connection goes, or when the command exits, the wrapper ends the whole session (TERM, then KILL after 5 s). |
| 1 | P2 | `run` could wait without limit | Fixed. A timeout is required. |
| 1 | P2 | `run` answered `Ok` when the transport failed | Fixed. The answer is `Lost`, with the command's stderr. |
| 1 | P3 | The remote socket was left behind | Fixed. It lives in a private `mktemp -d` directory that is removed. |
| 1 | P3 | The cleanup test checked less than its name said | Fixed. The tests now check the machine's actual processes and directory. |
| 1 | P3 | An outright kill leaves the master and its key behind; keys are never revoked | Documented as a limit. |
| 1 | P3 | The `Compression` claim was false through the master | Removed. |
| 1 | P3 | Forward paths were not checked; the host key was only checked as base64 | Fixed. `SocketForward::new` checks paths, and the ed25519 key blob is checked. |
| 2 | P2 | A command that ignores TERM survived | Fixed. KILL follows after 5 s. Tested with `trap '' TERM`. |
| 2 | P2 | Processes outside the command's process group (`timeout`) survived | Fixed. The wrapper now matches on session id. Only `setsid` escapes it, and that is documented. |
| 2 | P3 | A signal reads as exit 128+n | Documented. |
| 2 | P3 | A command starting with `-` | Fixed. `sh -c --`. |
| 2 | P3 | The wrapper could silently do nothing | Fixed. It refuses to run without `/proc` or when it does not lead its session. |
| 2 | P3 | Each stream or run is a new sign-in, with no cap on concurrent ones | Documented. A cap would belong in the caller. |
| 2 | P3 | The key directory was 0755 | Fixed. It is 0700. The reviewer found three leaked test directories, which were removed. |
| 2 | P3 | Drop blocked the runtime; command stderr could reach errors; the first line had no time limit | Fixed. |
| 3 | P3 | A client lost before the wrapper starts leaves it reparented, and the watcher would never fire | Fixed. The wrapper refuses to run unless its parent is sshd. Tested. |
| 3 | P3 | The 5 s grace needs a `sleep` that takes fractions | Documented. |
| 3 | P3 | A PID could be reused between the read and the kill | Accepted and documented; a shell has no pidfd. |
| 3 | P3 | A test asserted straight after SIGKILL | Fixed. It now polls for the end state. |
| 3 | P3 | Cleanup counts against `run`'s timeout | Documented. |

## Limits that remain

- A client cut off by a network partition is noticed only when sshd notices.
- An IPv6 address's scope is not kept.
- csh login shells are not supported.

## Evidence

- `cargo test --features boat --lib connection -- --include-ignored`: 12 passed, run twice.
  - The ignored tests need `/usr/sbin/sshd`, `python3` and an account named `user`. In the cloud sandbox:
    - `apt-get install openssh-server`
    - `useradd -m user`
    - `usermod -p '*' user`
- clippy and doc with `-D warnings`: clean.
- The full suite has one failure, `runtime_pty_adversarial::terminal_path_search_skips…`. It fails in this sandbox on the base commit too.
