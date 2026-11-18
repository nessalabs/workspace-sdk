# SSH keys off disk (#20): quality and adversarial review

Decision: **PASS** from both independent reviewers. No P1/P2 finding is
open.

## Scope

- `src/providers/openssh/agent.rs`: an ssh-agent served from this process
  on `<connection directory>/agent`, for one ed25519 key. ssh runs with
  `-i <dir>/key.pub`, `IdentitiesOnly=yes`, `IdentityAgent=<dir>/agent`.
- `src/providers/openssh/keys.rs`: keys made in memory (`getrandom` and
  `ed25519-dalek`, serialized as `openssh-key-v1`), and loaded keys parsed
  in full, refusing encrypted or mismatched ones. `ssh-keygen` is no longer
  run outside tests.
- `src/providers/openssh/directories.rs`: each connection directory holds an
  `flock`ed `lock`. Once per process, a sweep on a blocking thread removes
  this account's `/tmp/wsdk-ssh-*` directories that are older than 60 s and
  whose lock can be taken (or, for a directory made by an older build,
  whose master does not answer a non-blocking connect).
- The tunnel's `ProxyCommand` loses a second `exec`: ssh already runs it as
  `exec <command>`, and `exec exec nc` fails when the user's shell is bash.

## Commands and results

- `scripts/quality-gate.sh` (Ubuntu 24.04, run as root, `CARGO_INCREMENTAL=0`):
  every step passes but one PTY test,
  `runtime_pty_adversarial::terminal_path_search_skips_a_non_executable_shadow_before_a_valid_program`,
  which fails the same way on `origin/main` in this environment (root can
  run a file without its execute bit). The remaining steps were run with
  that one test skipped and pass, performance samples included.
- `cargo test --features boat,agent37 --lib openssh -- --include-ignored`
  with a local `sshd` and an account named `user` (Ubuntu 24.04, OpenSSH 9.6):
  every test passes, including the tunnel test and
  `a_killed_process_leaves_no_key_and_its_directory_is_swept`. That test
  SIGKILLs a process mid-connection, finds no file holding a private key,
  and sees the next sweep remove the directory.

## Reviewers

An independent code-quality reviewer (three rounds) and a separate
adversarial reviewer (two rounds), subagents, read only. Both pass the
final scope.

| # | Sev | Finding | Disposition |
| --- | --- | --- | --- |
| Q1 | P2 | The key format was checked in two places; `KeyPair::new` accepted keys that could never sign in | Fixed: `keys.rs` owns the parse; `Signer` takes a `KeyPair` |
| Q2 / A-P3 | P2 | The kill test swept `/tmp` with age zero and could remove a parallel test's new directory | Fixed: it back-dates its own directory and sweeps with 60 s |
| Q-r2 | P1 | Default-feature clippy failed: the key-loading path was unused without `agent37` | Fixed: `cfg_attr(not(feature = "agent37"), allow(dead_code))` on it, as `Dial::Tunnel` does |
| A1 | P2 | `KeyPair::generate` had `ssh-keygen` write the key to `$TMPDIR` for a moment | Fixed: keys are made in memory |
| Q3 / A-P3 | P3 | The sweep blocked async `connect`, and a legacy master with a full backlog could block it for ever | Fixed: `spawn_blocking`, nothing waits; non-blocking connect |
| Q4 | P3 | Two caps for one bound (semaphore and `JoinSet`) | Fixed: `clients.len()` |
| Q5 / A-P3 | P3 | No `# Errors`; a silent client held a slot for ever | Fixed: documented; 10 s per request read |
| A-P3 | P3 | A client that never reads blocked a reply write for ever | Fixed: the write has the same 10 s bound |
| A-P3 | P3 | The base64 copy of the key was not wiped | Fixed: `Zeroizing` |
| Q6, Q7, Q8, Q10, A-P3 | P3 | Docs: claims too broad, drop order, `/tmp` twice, long lines; `KEY_TYPE` twice | Fixed |

## Residual limitations

- Any process of the same account can ask the agent to sign while a
  connection lives, as it could read the old key file. The socket is in a
  0700 directory.
- A process killed outright leaves its ssh master running, holding its
  session but no key, until the machine or the network ends it; the
  directory (socket, public key, learnt host key) stays until the next
  process's sweep.
- The private key file is still handed to the application by
  `Agent37Identity::private_key`, for it to store. Where it keeps it is the
  application's choice.
