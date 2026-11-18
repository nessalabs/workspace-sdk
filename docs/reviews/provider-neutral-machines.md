# Provider-neutral machines (#17, #18, #19): quality and adversarial review

## Scope

- `src/domain/machines.rs`: the values every cloud answers in:
  - `MachineRequest`;
  - `Lifetime` (Unlimited, Hard, Idle) and `Backstop` (HardLifetime, or IdleSleep within bounds);
  - `Proven` (the cloud's identity for what a machine runs, plus an optional digest);
  - `Made`, `Disposition` (Remove or Retain), `Refused`/`RefusalCause`, `StopReason` and `MachineError`.
- `src/ports/i_machines.rs`: the `IMachines` port (`backstop`, `prepare`, `create`, `ready`, `stop`, `remove`), and `ICreationSite`/`Idempotency`/`Submitted` for creation.
- `src/providers/creation.rs` (`find_or_create`): one creation algorithm,
  beside the adapters that use it, so providers never depend on the
  application layer. A Native cloud only submits, and a lost answer is uncertain. A Tagged cloud finds, then submits, then finds again after a lost answer, and reports two tagged targets as `Ambiguous`.
- `Agent37Machines::create` and Boat's `admission::create` now go through it. Their behaviour is unchanged, with one exception: when a lost Agent37 answer turns out to have made a duplicate, the answer is now `Ambiguous` instead of `ProvisioningUncertain`.
- `BoatCloud` and `Agent37Cloud` adapters. Both pass one shared contract (`src/ports/i_machines_contract.rs`) against their HTTP fakes.

The existing `BoatMachines` and `Agent37Machines` APIs are unchanged, so nessa-agent#1356 builds as it is.

## Commands and results

- `cargo clippy --locked --all-targets -D warnings` passes with `--all-features`, default features, `--no-default-features`, `--features agent37` and `--features portability`.
- `cargo test --features boat,agent37 --lib` passes. That includes 8 algorithm tests (lost answer, duplicate, find failure before and after a lost answer, native path, certain refusal), both clouds' contract tests, and the unchanged Boat and Agent37 suites. Agent37's "still being made" case is `an_instance_still_being_made_is_not_judged_yet`.
- The full gate is in the pull request.

## Reviewers

An independent code-quality reviewer (Q) and a separate adversarial
reviewer (A), subagents, read only.

| # | Sev | Finding | Disposition |
| --- | --- | --- | --- |
| A1 | P3 | `Lifetime::Hard(Duration::MAX)` overflowed when rounding up | Fixed: `checked_add`; tested |
| A2 | P3 | `Proven` said both parts are compared exactly; Agent37 never re-reads its revision | Fixed: the doc says the revision is kept for records only |
| A3 | P3 | The contract accepted `InvalidInput` for a foreign proof | Fixed: it requires `AdmissionRejected` |
| A4 | P3 | The Boat removal test's comment claimed what it did not test | Fixed: the comment says what the fixture does |
| A5 | P3 | `Unlimited` on Agent37 still sleeps after 24 h | Fixed: documented on `Lifetime::Unlimited` |
| A6 | P3 | Agent37 rounded idle down, while Boat rounds lifetimes up | Fixed: rounded up; tested |
| A7 | P3 | `CreationFailure::Site` doc too narrow | Fixed |
| A8 | P3 | `Agent37Cloud::remove` didn't check that a `Made` was Agent37's | Fixed: it refuses another cloud's proof; tested |
| Q1 | P1 | `removable` was unused without `boat`/`agent37`, so the gate's other clippy runs failed | Fixed: `Made::removable` in the domain; all six clippy runs pass |
| Q2 | P2 | Providers imported `crate::application::find_or_create`, inverting the layers | Fixed: moved to `providers::creation` |
| Q3 | P2 | #19 is half met: the managed records' `domain::Admission`/`RefusalKind` stay Boat-shaped beside `Proven`/`RefusalCause` | Deferred, recorded on #19 with a follow-up issue: moving the saved records needs a serde migration |
| Q4 | P2 | The contract accepted two outcomes for a foreign proof | Fixed (as A3) |
| Q5 | P3 | `ICreationSite::find` required of a Native cloud | Fixed: default answers none |
| Q6 | P3 | Two `Disposition` types | Documented: the domain's names the mapping |
| Q7 | P3 | `Proven` said "save it" but could not be serialized | Fixed: serde with `try_from`, checked again on load; tested |
| Q8 | P3 | Agent37's duplicate-after-lost-answer is now `Ambiguous` | Kept: nessa-agent#1356 maps it as it maps `ProvisioningUncertain` |

## Residual limitations

- Creation on a Tagged cloud is safe only while attempts for one key are serialized. The SDK's per-workspace record does that within one process; across processes the caller must. Two tagged targets are reported, never chosen between.
- Agent37 has no hard lifetime. An open connection's keepalives keep an instance awake.
- Nessa still uses `BoatMachines` and `Agent37Machines` directly. Moving it onto `IMachines`, which also ends `EnvironmentIdentity`'s need for Boat UUIDs (#19), is follow-up work in nessa-agent once #1356 merges.
