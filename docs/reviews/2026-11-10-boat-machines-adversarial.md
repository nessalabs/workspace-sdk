# Boat machines (W3): adversarial review

Reviewer: a separate adversarial agent. It looked for races, unsafe retries,
boundary failures, leaks and tests that mislead. Scope and rounds match
`2026-11-10-boat-machines-quality.md`.

## Decision

PASS once the round 2 fixes are in. No P1 or P2 is open. The limitations listed
below are known and stated.

## Round 1 findings and what was done

| Sev | Finding | Disposition |
| --- | --- | --- |
| P1 | A spent budget gave an unbounded readiness wait | Fixed. See the quality record. |
| P2 | The budget did not bound HTTP calls outside the poll loops | Fixed. In `create`, the environment check (`Provider`), the POST and body read (`ProvisioningUncertain`), and the start wait and check (a `ReceiptUnavailable` refusal that keeps the VM named) each run under `timeout_at`. All of `ready` runs under one. `prepare` and `stop` are bounded by the caller's HTTP timeouts, as documented. |
| P2 | `ready` did not require an operable VM | Fixed. The final check requires `is_operable`. `Created` documents that admitted does not mean running. |
| P2 | An empty or malformed key was not refused | Fixed (`InvalidInput`). |
| P2 | `Debug` printed variable values | Fixed. Only names are shown. Test `a_spec_shows_variable_names_and_never_their_values`. |
| P3 | A definite 4xx was mapped to uncertain | Reverted in round 2. A 4xx on a retry says nothing about an earlier attempt with the same key, so every non-2xx answer stays uncertain. |
| P3 | A refused replay of the caller's own key is retained | Kept on purpose. This is the ported behaviour: a replayed VM may be in use. |
| P3 | `vm` was not checked before the URL | Fixed. |
| P3 | `Instant + Duration` could overflow | Fixed. |
| P3 | The sink-panic test covered only `create` | Fixed. It now covers `stop` as well. |
| P3 | `catch_unwind` does nothing under `panic = "abort"` | Documented on `IMachineEvents::record`. |

## Round 2 findings

| Sev | Finding | Disposition |
| --- | --- | --- |
| P2 | A 4xx on a retry was taken to mean no VM exists | Fixed. Every non-2xx answer is `ProvisioningUncertain`. Test `every_answer_without_a_vm_stays_uncertain`. |
| P2 | A restarted caller could not roll back | Fixed with `Created::restore`. |
| P3 | A budget already spent before the POST was reported as uncertain | Fixed. The deadline is checked before sending, and nothing is sent once it has passed (`Provider`). |
| P3 | The spent-budget test exercised the outer bound and not the `left == 0` guard | The test is renamed to say what it shows (`a_tiny_budget_ends_readiness_instead_of_waiting_without_end`). The guard is kept as defence in depth. |
| P3 | Right after a resume, the readiness wait may see `archived` and fail | Inherited from the original and not observed against Boat. Left open, to be checked on the first real Boat run (Nessa C1). |
| P3 | A timeout during a resume returns `Provider` while the VM may run | Documented on `ready`. The spec's lifetime bounds it. |
| P3 | No panicking-sink test for `roll_back` | Accepted. `roll_back` records through the same `Boat::record` that the `create` and `stop` tests cover. |

## Evidence

- `cargo test --features boat --lib machines`: 40 passed.
- clippy with `-D warnings` on all features: clean.
- `cargo doc` with `-D warnings`: clean.
- The full gate passes with one exception. `runtime_pty_adversarial::terminal_path_search_skips_a_non_executable_shadow_before_a_valid_program` fails in the cloud sandbox, and it fails on the base commit too, so this change did not cause it.
