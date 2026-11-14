# Agent37 provider: independent quality review

Decision: **QUALITY PASS**. No unresolved P1/P2 finding.

Scope: branch `claude/agent37-provider` (`git diff origin/main...HEAD`): the
OpenSSH connection moved to `src/providers/openssh/` and generalized over a
provider `Route`; Boat's key and host-key handling in
`src/providers/sandbox/ssh_route.rs`; the new `agent37` feature in
`src/providers/agent37/`; `tests/agent37_provider_contract.rs`.

Reviewer: an independent subagent, read only, three rounds.

## Findings and disposition

| # | Sev | Finding | Disposition |
| --- | --- | --- | --- |
| 1 | P2 | Attach-only provider held workspace-wide `Agent37Machines` (stop/delete) | Fixed: provider takes `Agent37Config`, holds only the API |
| 2 | P2 | Version/provider check and locator round-trip duplicated from Space | Fixed: shared `managed::decode_reference`; control-root helper left as follow-up |
| 3 | P2 | Implicit `key`/`hosts` file contract between openssh and providers | Fixed: `Route` carries a `KeyPair` and `Dial::Direct { endpoint, host_key }`; openssh writes both |
| 4 | P2 | Duplicated `ssh-keygen` and a looser public-key check | Fixed: `openssh/keys.rs` shared by Boat and Agent37 |
| 5 | P2 | Instance status as raw strings in six places | Fixed: `api::Status` enum with `at_rest`/`gone` |
| 6 | P2 | Transient codes defined twice | Fixed: one `TRANSIENT` set |
| 7 | P2 | Duplicate-key instances reported as retryable | Fixed: `Agent37Failure::Ambiguous` |
| 8 | P2 | No live test; private path never run | Partly fixed: ignored live test added and run (below); CI still cannot run it |
| 9-17 | P3 | Connection per private request, tunnel shutdown, naming, loopback-only plaintext, tokio::fs, bounded codes, cleanups | Fixed |
| 16 | P3 | No region setter or `Size` validation | Follow-up |
| new | P3 | Lock held while a link is made; doc drift on waking; hand-parsed URLs; exec retry source | Fixed |

## Evidence

- `scripts/quality-gate.sh` (now also clippy and tests for
  `--no-default-features --features agent37`): passed on the final commit.
- Fake-Agent37 tests: 17 library tests (machines, tunnel, idle slot) and 3
  contract tests in which `exec` runs the real supervisor on the host.
- Live, 2026-11-14, against api.agent37.com with the owner's approval
  (`cargo test --features agent37 agent37::live -- --ignored`): created an
  `agent37-base@2026.10.24a` instance in 0.66 s, connected through the
  tunnel in 3.0 s, ran a command in 2.3 s (`node`, `Linux x86_64`,
  `/home/node`), round-tripped bytes through a `-R` Unix-socket forward,
  stopped and woke it, and deleted it: 12.7 s in all.

## Remaining limitations

- The private path through a real tunnel and sshd runs only in the ignored
  live test. The ignored local-sshd connection tests need Linux (`/proc`).
