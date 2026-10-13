# Code quality and acceptance

## Design review

- Keep domain identity and invariants explicit. References must retain their
  target and lifetime; stale references must not silently address newer work.
- Depend on narrow, I-prefixed external interfaces. Infrastructure owns native,
  provider, transport, and storage details. Use existing capabilities instead
  of duplicating a dependency's implementation.
- Group related concepts in focused modules. Imports belong at module tops;
  module entry files primarily declare and export their contents. Document
  public ownership, bounds, cancellation, retry, and failure contracts.
- Prefer simple concrete code internally. Patterns must solve an observed
  coupling or lifecycle problem, not add interfaces or indirection for style.
- Split services by coherent responsibilities when necessary. A long-running
  setup action must not block unrelated preparations or hide a control request
  behind an unbounded lock wait.

## Adversarial review

- Exercise wrong targets, stale attempts, ambiguous transport responses,
  launch-before-save failure, cancellation races, and concurrent controllers.
- Confirm that process exit, authentication success, and uncertainty remain
  distinct. Never erase uncertain work merely to permit a retry.
- Bound input, output, waiting, replay, allocation, and background ownership.
  Partial input and unknown delivery are outcomes, not permission to replay.
- Keep transient credentials out of ordinary logs, Debug, and receipts. Terminal
  echo and child output can expose input; do not promise output sanitization.
- Test behavior at real boundaries. Fake hosts establish orchestration, real
  local fixtures establish process behavior, and live remote tests establish
  provider behavior. None substitutes for the others.

## Performance and evidence

Run the local performance qualification at each gate. Record actual samples and
environment; do not call a few samples a statistical regression baseline. Add
relevant input/resize/cancel, concurrency, memory, and throughput scenarios when
the implementation supports them. A documented unsupported scenario is not a
passed scenario. See `docs/performance.md` for commands and current coverage.

Each review record names the reviewed changes, commands/results, independent
reviewers, severity and disposition of every actionable finding, re-review
decision, and residual limitations. Both independent reviewers must explicitly
pass the final scope. Never suppress a finding to obtain a green gate.
