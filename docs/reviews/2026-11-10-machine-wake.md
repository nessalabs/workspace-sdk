# Checking a Boat machine on every wake: independent reviews

Two separate read-only agents reviewed this: one for quality, one adversarial. Scope:
- `sandbox/locator.rs`: the machine's spec and admission saved in its reference.
- The machine branch of `SandboxTarget::ensure`.
- `sandbox/refusal.rs`.

Commits reviewed: f4e9bf9 and 68a0f35.

## Decision

Both PASS at 68a0f35. No P1 or P2 is open.

## Findings and dispositions

| Review | Round | Sev | Finding | Disposition |
| --- | --- | --- | --- | --- |
| Adversarial | 1 | P2 | After a restart, a stopped machine could never wake. Ownership was proven by reading a marker inside the VM, which cannot be read while it is archived. The tests started out already verified. | Fixed in 68a0f35. An attached reference is refused first, `ready` then wakes and checks the machine, and the marker is verified on the running VM. The tests now start unverified and include a foreign owner. |
| Adversarial | 1 | P3 | `ready` ran twice on create, so the second run could refuse a VM that creation had just made | Narrowed. `open`'s first wake skips `ready`. The SDK's `ensure_ready` right after still checks again; if that refuses, the record is already Ready and its owner can destroy the machine. |
| Adversarial | 1 | P3 | A reference with its machine removed would wake without a check | Fixed. A provider with machines refuses an SDK-owned reference that names no machine. |
| Quality | 1 | P3 | `target` and `provider` called into each other | Fixed. The error mapping now lives in `sandbox/refusal.rs`. |
| Quality | 1 | P3 | `refuse` built a throwaway `Machine` | Fixed. `locator::admission(&EnvironmentIdentity)` replaces it. |
| Quality | 1 | P3 | A refused machine found running is left running, and nothing said so | Documented on `from_machines`. |
| Quality | 1 | P3 | `locator::decode` copies `managed::decode`'s checks | Accepted. They are the same three checks, both reading one validator. |
| Adversarial | 2 | P3 | A forged reference naming another workspace's archived VM resumes it, then fails ownership and is left running until its TTL | Accepted. Bounded by the TTL, and it needs a forged reference on the same account. |
| Adversarial | 2 | P3 | Switching a provider ID from plain to machines strands its old SDK-owned workspaces | Documented on `from_machines`. |
