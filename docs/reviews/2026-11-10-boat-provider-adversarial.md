# Boat machines in SandboxProvider: independent adversarial review

Reviewer: a separate adversarial agent, read-only. Scope: `SandboxProvider::from_machines`, `create_machine`, the refusal mapping, the wake budget in `SandboxTarget::ensure`, and `AdmissionRefusal::WipePending`.

Rounds: b6a3d31, 042fd52 and 48d7713. Its last P3 was fixed in 5d811bf.

## Decision

PASS at round 3. No P1 or P2 is open.

## Findings and dispositions

| Round | Sev | Finding | Disposition |
| --- | --- | --- | --- |
| 1 | P2 | A replay (`RetainTarget`) of this record's own VM, still starting, was refused for good (`WipePending`, `ReceiptUnavailable`), which used up the workspace ID | Fixed. Only causes the receipt makes certain are final (`final_kind`). The rest leave the record pending. |
| 1 | P2 | `ReceiptUnavailable` covers any transient error, yet it was final | Fixed. It can be retried, and so can `WipePending`. |
| 1 | P2 | A VM that `ready` resumed and then failed kept running | Fixed. `ready` stops a VM it woke (`StopCause::NotReady`). |
| 2 | P2 | The fix stopped replayed VMs without condition, which could stop another caller's live machine | Fixed. A create never stops a replay. `ready` stops only what it resumed itself. |
| 2 | P3 | `EnvironmentChanged` cannot pass later, because the retry reads a new version | Fixed. Final, as `EnvironmentMismatch`. |
| 2 | P3 | A stop failure was not reported | Fixed for the retryable path: the error says whether the stop was confirmed. |
| 2 | P3 | Stops were recorded as `Closed` | Fixed. `NotReady` and `NotAdmitted` were added. |
| 1 | P3 | The wake budget overran and its errors were mixed | Fixed. A single `timeout_at` covers every request and wait, and ends in `Timeout`. |
| 1 | P3 | A Boat environment name in the message could break markdown quoting | Fixed. It is quoted with `Debug`. |
| 1 | P3 | `Missing` pointed at only one cause | Fixed. The message names both and points to `forget_unfinished`. |
| 1 | P3 | The docs on variables overstated what is saved | Fixed. They are sent to Boat, hashed into the record and printed by `Debug`. |
| 3 | P3 | A fresh VM that failed `ready` for a non-admission reason kept running | Fixed in 5d811bf. |

## Left open, accepted

- A later wake does not admit the machine again. The docs say admission covers creation only. A follow-up slice will save the spec with the workspace so `ready` can wake it.
- A fresh VM whose check may pass later becomes a replay when retried. A final refusal after that keeps it, and the VM's lifetime bounds the cost. A pending wipe that never clears loops on each retry the caller makes.
- `ProvisionKey::for_workspace` is not salted per deployment. That is why a replay is never deleted or stopped by a create.
- `ready`'s stop after its own resume may be refused while Boat is still resuming. Its result is not reported.
