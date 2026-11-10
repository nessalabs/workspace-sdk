# Boat machines (W3): independent quality review

Reviewer: separate code-quality agent. Scope: `src/providers/sandbox/machines/`,
which is Nessa's Boat VM logic (`boat.rs`, `boat_admission.rs`,
`boat_receipt_diagnostics.rs` and their tests) ported into the SDK. The reviewer
read `coding_standards.md`, `AGENTS.md`, the ported code and the originals.
Round 1 reviewed 04db2bd. Round 2 reviewed 3abea16. The fixes after that are in
the following commit.

## Decision

PASS once the round 2 fixes are in. No P1 or P2 finding is left open.

## Round 1 findings and what was done

| Sev | Finding | Disposition |
| --- | --- | --- |
| P1 | `ready` waited without end once the budget was spent (the client reads a zero wait as no deadline) | Fixed. All of `ready` runs under `timeout_at`. A spent budget returns `Provider` before the client wait. `Budget::new` refuses zero. |
| P2 | Org scope was not forced to personal | Fixed. `new` applies `with_org(personal)`, and raw requests always send `X-Boat-Org: personal`. Test `every_call_is_made_in_the_personal_scope`. |
| P2 | A malformed key was reported as `ProvisioningUncertain` | Fixed. The key must be 1–255 ASCII-graphic bytes, otherwise `InvalidInput`. |
| P2 | `MachineSpec` was not validated | Fixed. `MachineSpec::new` checks it and the fields are private. Tests are in `spec_tests.rs`. |
| P2 | `Instant + Duration` could overflow | Fixed. `MAX_BUDGET` is 24 h and the deadline uses `checked_add`. |
| P2 | mod.rs held the implementation, and the modules depended on each other in a cycle | Fixed. The code is split into `spec.rs`, `events.rs` and `account.rs`. mod.rs only declares and re-exports. |
| P2 | `roll_back(vm)` could delete a retained VM | Fixed. It takes `&Created` and refuses `RetainTarget`. |
| P2 | Public API contracts were missing | Fixed. Docs now cover the timeouts, the budget's scope, the errors, the variants and the fields. |
| P3 | `vm` was not checked before going into the URL | Fixed. `vm_id` is checked in `ready`, `stop` and `Created::restore`. |
| P3 | The tests module sat before `decode`, an inline `super::` path was used, and helpers were too visible | Fixed. |
| P3 | There were two public paths | Not a problem. `sandbox` is a private module, so `boat_machines` is the only path. |
| P3 | Test names used the source application's names | Fixed. Neutral names are used. |
| P3 | `prepare` can leave a newly made environment stuck in the default posture | Documented. `prepare` never changes an environment it did not just make. |

## Round 2 findings

| Sev | Finding | Disposition |
| --- | --- | --- |
| P2 | A restarted caller could not build a `Created` to roll back | Fixed. `Created::restore(vm, admission, disposition)` is checked with `vm_id`. Test `a_saved_creation_can_still_be_rolled_back_after_a_restart`. |
| P2 | A 4xx on a retried create could lose a VM | Fixed. Every non-2xx answer stays `ProvisioningUncertain`, as in the original. |
| P3 | `vm_id` is looser than the client's `bx_` rule, so some bad ids surface as `Provider` | Accepted. The vendored client takes no source patches and keeps its validator private. `vm_id` still keeps every id safe in a path. |
| P3 | A budget timeout after a resume can leave the VM running | Documented in the errors section of `ready`. |
