# Recovery at an expected checkpoint

Use `RecoveryService::recover_at_head` when a caller has selected an exact
acknowledged `RecoveryHead` and must refuse a newer canonical checkpoint.
Pass that head alongside the ordinary `RecoveryRequest`.

The service checks the head while holding the existing recovery effect lease,
before preparing or launching an attempt. An epoch check alone cannot detect a
publication that changes the canonical head within the same execution epoch.
A mismatched head returns `Conflict` without advancing intent or opening a target.

Keep the request and expected head unchanged across retries. An exact retry
compares the expected head with the attempt's original pinned checkpoint, so it
can return the accepted run after that run publishes a newer canonical head.
Changing the expected head on that retry conflicts. Retained-workspace attempts
still use `continue_current`; the fresh and retained entrypoints cannot substitute
for one another.

The ordinary `recover` API keeps its existing behavior. Both methods share the
same owned-task lifetime, repository lease, revision CAS, native launch claims,
confirmed-stop safety and uncertainty handling. This precondition is not a global
filesystem fence, a durability guarantee for remote storage, or an exactly-once
guarantee for external effects.
