# Stopped handoff receipt recovery: adversarial acceptance review

Reviewer: independent agent `workspace_handoff_adversarial_review`.
Decision: **PASS for the three new local stopped-handoff regressions.** No
unresolved P1/P2 or other actionable finding was found in this change. This is
a test-only acceptance decision, not a new production failover guarantee.

## Reviewed source and evidence

Base: `0b7bbc0a9be0cce0713df05be56f2a63d6c14c30`, branch
`codex/workspace-portability`. The reviewed implementation change is solely
`tests/sdk_workflow.rs`, SHA-256
`26b6da60d7365932a5405839aa62a6cfec28f606dd168b49329f4266f8c0d4e6`.
The complete uncommitted binary diff before review records was SHA-256
`ba1b07344214f5098e56d2e2b26e4ff7fd82252e47b0889c546887a5ae030c9b`.
I independently matched both hashes and inspected the actual diff, repository
instructions, workflow repository, handoff orchestration, local provider,
managed launch bridge, native supervisor claim ordering, run waiting, and native
workload fixture. Production is unchanged.

I inspected the parent's completed `scripts/quality-gate.sh` evidence in
`../workspace-tools-handoff-quality-gate-corrected.log`: both feature
configurations, formatting, Clippy, documentation, managed-process samples and
PTY qualification completed. Both workflow-suite runs passed all 10 tests.
The final restored run, `cargo test --locked --test sdk_workflow`, passed all
10 in 3.08 seconds (`../workspace-tools-handoff-restored-tests.log`). The
initial performance-report destination setup failure is not a successful gate;
the corrected completed run supplies the evidence. Cached object-stripping
warnings remain in its log. Three managed samples and 15 PTY samples are local
qualification, not a statistical regression baseline or a zero-warning claim.
I did not rerun builds or mutation probes during this independent review.

The committed [qualification evidence](../evidence/handoff-recovery-2026-10-17/qualification.json)
now preserves the source identity, actual performance samples and environment,
restored workflow result, fault panic bodies, and setup/warning limitations.
I independently decoded the raw completed gate log and matched both complete
performance objects exactly (3 managed samples and 15 PTY samples), matched all
three probe records and panic bodies field-for-field to the raw proof, and
checked the 10/0 restored result and unchanged test-source digest. No discrepancy
was found. This is a self-contained evidence summary; it does not expand the
acceptance scope or exclusions below.

## Boundary and adversarial assessment

1. **Restored publication before launch.** The wrapper delegates the real
   workflow lease, ordinary loads, capture publication and unaffected saves.
   Its single selected Restored save returns the exact injected error without
   writing. The destination already contains restored counter value 3, but the
   test independently examines native run directories and requires no
   `claim.json`. After dropping all workspace/run/controller/repository handles,
   a new FileStateRepository reads Captured from disk. A new SDK reconnects
   the original target references and resumes to counter 5. This checks the
   filesystem-apply/receipt boundary, rather than assuming an error prevented
   all earlier effects.
2. **Started write failure after actual launch.** The attempted receipt carries
   the real workspace-qualified destination RunRef. The test waits for its
   native continuation to exit successfully and observes counter 5 before
   retry. Disk still contains Restored. Recovery must return exactly the same
   RunId and workspace, then repeated handoff returns that same reference and
   leaves counter 5. The code trace confirms the stable operation-derived
   launch ID reaches the supervisor's fingerprinted durable claim; an existing
   claim returns inspection rather than launching another process. This is not
   a fake host that merely counts calls.
3. **Started acknowledgement loss after commit.** The wrapper invokes the real
   fsync-backed repository save successfully before returning the injected
   error. The fresh repository must read Started. Recovery returns the
   historical run reference. The incompatible continuation is rejected even
   for that already-completed phase, and its distinct `unexpected` effect must
   remain absent. In every scenario the same incompatible retry is tested
   before a valid retry; counter and absent-file assertions guard against an
   error result that nevertheless executed that command.

The wrapper records the attempted receipt before injecting its fault, but that
memory cannot substitute for persisted recovery state: the wrapper is dropped,
phase assertions use a newly constructed disk repository, and the fresh
SyncService receives that ordinary repository. Matching phase by discriminant
is sufficient for these fieldless enum variants. The one-shot fault is scoped
to the chosen operation and phase. A failure to reach it cannot silently pass:
the initial exact error, captured attempt, on-disk phase and subsequent effects
must all agree.

The counter workload performs a real read and write in the destination project.
All successfully started workloads are awaited to terminal success before
teardown, each test owns a unique temporary tree, and different tests therefore
do not share operation IDs or targets. No arbitrary sleeps substitute for
launch/exit observation. Repeated Started receipt reads do not start a new
workload in the inspected code. The source counter remains 3; this establishes
that selected source data is unchanged, not a complete source-tree comparison.

## Deliberate-break evidence

I inspected the actual mutation runner and all three recorded runtime panic
bodies in `../workspace-handoff-probes-proof.json`:

- Moving Restored publication after launch fails the native no-claim assertion.
- Replacing the deterministic destination launch ID with a fresh random ID
  fails the exact recovered-versus-attempted RunId assertion.
- Bypassing handoff input identity validation fails the incompatible historical
  retry's required Conflict assertion.

Each ran exactly its intended test, exited 101 at the intended assertion,
restored the original production bytes and frozen complete diff, and was
followed by the restored 10-test green suite. Compile errors, watchdog expiry
and setup failures are not counted as enforcer proof. No finding required a
fix or re-review in this independent review round.

## Explicit exclusions and limits

These are synchronous repository-boundary fault injections with real local
storage/process effects. They do not kill the controller or machine, corrupt
the state file, fail an actual fsync/rename, simulate power loss, or qualify
remote transport failure. Controller replacement occurs after the observed
continuation has exited; recovery while that continuation is still running is
not newly exercised. The checkpoint engine object is retained, although
workflow repository, SDK and backend handles are recreated.

No automatic disconnect detection, source cancellation, VM memory migration,
agent conversation migration, concurrent-controller race matrix or provider
failover is established. The source is confirmed terminal before handoff, the
destination begins empty, and quiescence is asserted by the caller. Concurrent
external writers and arbitrary continuation side effects are outside these
three tests. The final counter check is a concrete sequential once-only effect
oracle, not a proof of exactly-once arbitrary external effects or every possible
concurrent duplicate-launch schedule. Existing native claim code provides the
traced retry mechanism; these tests do not newly fault every instruction in it.

Waits are bounded through existing Run::wait and local supervisor invocation
limits. These small tests do not qualify maximum archive size, receipt-store
exhaustion, memory throughput or cancellation cleanup under assertion failure.
On successful paths started processes are terminal before temporary-directory
cleanup. A failed assertion is not evidence of guaranteed process cleanup.
These exclusions do not block the narrow test-only change and must remain
attached to any shipment description.
