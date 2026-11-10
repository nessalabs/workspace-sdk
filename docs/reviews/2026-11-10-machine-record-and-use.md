# Machine record and counted use review (W2, W5)

Date: 2026-11-10. Branch `claude/project-thread-z6wavf`, pull request #9.

Scope:
- W2 makes each workspace ID hold one saved machine record. Create and attach are separate calls. The record moves from Provisioning or Attaching to Ready or Refused and never back. MemoryStateRepository moves behind the `testing` feature.
- W5 adds `Machines`: named uses of SDK-owned machines and one rest path.

## Round 1

Two separate agents reviewed `a968b79..fa31c66` (W2 only):
- a design reviewer applying Clean Code, SOLID, dependency direction and cohesion;
- an adversarial reviewer looking for races, unsafe retries, boundary failures and misleading tests.

### Adversarial findings

| ID | Finding | Resolution |
| --- | --- | --- |
| P1-1 | Forgetting a record while a call is in flight let that call finish on the next record, because revisions restarted at 1. Create's resume path never checked ownership, so a create could return an attached target as `OwnedWorkspace`. | Fixed. Revisions never repeat for an ID: the store keeps the last revision of a forgotten record (`retired`), and a new record starts above it. `settle` reports a forgotten record as a conflict. Every create result goes through `OwnedWorkspace::from_owned`. `forget_unfinished` takes the revision the caller read. Tests: `a_forgotten_request_cannot_land_on_the_next_record`, `forgetting_an_unfinished_create_frees_the_id`. |
| P1-2 | Legacy unfinished creates could never be retried after the upgrade, because request hashes changed shape. Legacy attaches were recorded as creates. | Fixed. Upgraded records carry `RequestOrigin::Upgraded` instead of an old hash. The first create or attach that fits adopts the record. A pending one takes the new request's initial state. A Ready one is kept only if its ownership matches the request kind. Tests: `an_upgraded_reservation_is_finished_by_the_next_create`, `an_upgraded_ready_target_is_kept_by_a_fitting_request_only`, and the file repository's legacy test. |
| P1-3 | The ID was reserved before the provider validated the request, so a typo pinned the ID. | Fixed. `IProvider::check_create` and `check_attach` have no side effects and run before `begin`. Each built-in provider implements them, and its create and attach also call them. SPACE refuses create there. Test: `a_request_the_provider_rejects_reserves_nothing`. |
| P2-1 | A refusal could override a Ready record saved first by another caller. | Fixed. Both the Refused and the Ready branch return the winner's saved outcome. Test: `the_first_outcome_saved_wins_for_every_caller`. |
| P2-2 | A reconnect could replace the saved admission. | Fixed. `MachineState::allows` refuses Ready to Ready unless reference, ownership and admission are all equal. `MachineRecord::refresh` always keeps the saved admission. Ready to Refused needs the refusal's admission to equal the saved one. Tests: `a_saved_target_is_never_rebound`, `a_ready_target_is_refused_only_under_its_own_admission`. |
| P2-3 | A retry "with the same key" assumes the provider keeps idempotency keys forever. Boat's key lifetime is undocumented. | Accepted limitation, owned by W3. The Boat port (workspace-sdk#10) adds `roll_back` for a created target whose record could not be saved. Until then, `forget_unfinished` is documented as safe only after the provider confirms it holds nothing. |
| P3 | `Workspace::backend()` still exposes `stop` and `destroy`. | Accepted for now. Built-in backends refuse both for attached targets at run time. W6 splits IBackend into capability traits, where `Lifecycle` is only reachable from an owned handle. |
| P3 | The v4 decoder accepted invalid records. | Fixed. `MachineRecord::validate` runs on every decoded record. It checks that the revision is at least 1, a Ready descriptor belongs to the record's ID, and a reconnected record is never pending. |
| P3 | The forget test showed forgetting without provider confirmation as normal use. | Fixed. The test now checks the revision guard, and the method doc says when forgetting is safe. |

### Design findings

| ID | Finding | Resolution |
| --- | --- | --- |
| P2-1, P2-2 | Same as adversarial P1-2. | Fixed as above. |
| P2-3 | Refusal and admission have no production producer. | Accepted, with an agreed producer. The Cloud VM thread's W3 maps Boat's environment mismatch, pending wipe and not-admitted outcomes to `Error::Refused` and fills `admission`. The types are the contract that work builds on. |
| P2-4 | The repository port promised a rule only the built-in stores followed. | Fixed. `MachineRecord::begin`, `advance`, `refresh` and `check_forget` decide every change in the domain. Stores only make each call atomic. The port doc now says so. |
| P3-1 | The rebinding check existed twice. | Fixed. Only `MachineState::allows` decides it. |
| P3-2 | The create/attach check in `finish` was dead code inside a closure. | Fixed. `finish` takes the provider future, checks the pending kind once against the record's state, and has no closure. |
| P3-3 | `forget_provisioning` was misnamed and its guard checked nothing. | Fixed. It is now `forget_unfinished(id, expected_revision)`. |
| P3-4 | `OwnedWorkspace` trusted the provider's ownership claim. | Fixed. `connect_owned` and `owned_workspace` need a saved Ready record that says SdkOwned with the same reference. The public `TryFrom` is gone. |
| P3-5 | There were three ways to reach the inner workspace. | Fixed. Only `Deref` and `From` are left. |
| P3-6 | Tests were missing. | Fixed. `tests/machine_record.rs` covers the race winner, create and attach on one ID, the attach retry, rebinding, refusal under admission, both legacy upgrades, and forgetting while a call is in flight. |
| P3-7 | Refusal Display showed a Debug name. | Fixed. `RefusalKind` has its own Display. |

## W5: counted use and rest

`Machines` owns one entry per machine:
- a tokio lifecycle mutex holds the awake handle;
- a short sync mutex holds the named uses and when the last use ended.

`hold` and `rest_idle` both take the lifecycle lock. A rest therefore never stops a machine that a new use has just opened. Dropping a `MachineUse` only removes its name and, when none is left, starts the idle clock. Nothing reads reference counts.

Tests in `tests/machine_use.rs`:
- a held machine never rests;
- an idle machine rests once;
- a new use wakes a rested machine and restarts the clock;
- the background rest reports what it stopped;
- an attached machine cannot be held for rest.

## Round 2

Pending. Results are added here after the re-review.

## Remaining limitations

- Boat key lifetime and rollback of a created target are W3's (adversarial P2-3).
- `Workspace::backend()` exposes lifecycle methods until W6.
- A `Machines` entry is kept for each machine it has seen. A caller that holds very many short-lived machines grows the map. Removing an entry when its machine is destroyed comes with W6's lifecycle trait.
