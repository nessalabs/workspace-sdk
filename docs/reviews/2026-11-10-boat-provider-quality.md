# Boat machines in SandboxProvider: independent quality review

Reviewer: a separate code-quality agent, read-only. Scope is the same as the adversarial review: b6a3d31, 042fd52, 48d7713 and 5d811bf.

## Decision

PASS at round 4 (5d811bf). No P1 or P2 is open.

## Findings and dispositions

| Round | Sev | Finding | Disposition |
| --- | --- | --- | --- |
| 1 | P2 | The docs said "admitted in full", but a later wake does not admit the machine again | The docs now say admission covers creation only. Re-admission on wake is a follow-up slice. |
| 1 | P2 | A replay that `ready` resumed and then refused kept running | Fixed. `ready` stops what it woke. |
| 1 | P2 | `ReceiptUnavailable` was a final refusal | Fixed. It can be retried. |
| 1 | P2 | No tests for the success path or a `ready` rejection | Fixed. Added tests for admission carried, a replay resumed and refused, a retry after a wipe ending admitted, and a replay left running. |
| 2 | P2 | Replayed VMs were stopped without condition | Fixed, as in the adversarial review. |
| 3 | P2 | The wipe retry could loop for good: Boat may keep the flag on an archived VM, and the retry never woke it | Fixed in 5d811bf. A pending wipe goes to `ready`, which lets a stopped VM through its check before waking, and resuming ends the wipe. The fixture keeps the flag until a resume. |
| 1 | P3 | `with_machines` threw away the client given to `new` | Fixed. `from_machines` replaces it. |
| 1 | P3 | `EnvironmentUnavailable` was counted as a mismatch | Fixed. It is now `NotAdmitted`, and the docs say a posture fixed later does not save the machine. |
| 1 | P3 | The wake errors were mixed | Fixed. |
| 3 | P3 | `StopCause` gained variants | Fixed. It is `#[non_exhaustive]`. |
| 3 | P3 | Another caller may race `ready`'s stop | Documented on `ready`. |
| 1 | P3 | The spec is built twice, `ensure` repeats after `ready`, and `with_wake_budget` takes a `Budget` | Accepted. Building the spec is cheap, and `ensure` installs the supervisor. |
| 4 | P3 | When `before_waking` lets a VM through on a pending wipe, the receipt checks after that field have not run, so a VM that is wrong in another way is briefly woken before the check after it is ready refuses and stops it | Accepted. It is never used, and `ready` stops it. |
| 4 | P3 | When `ready` fails on a fresh VM, the result of the stop is not reported | Accepted. It is the same as the stop `ready` makes itself. |
