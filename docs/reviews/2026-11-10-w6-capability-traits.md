# W6 review: capability traits

Change: workspace-sdk#12, commit d84833a. It does three things:

- It splits `IBackend` into a core, plus capability traits handed out as `Option<&dyn Trait>`.
- It derives `Capabilities` from those accessors.
- It keeps lifecycle and deletion away from attached targets and from shared handles.

Two separate agents reviewed it independently, one for design and one adversarially.

The adversarial reviewer ran these checks:

- `cargo check --all-targets` on six feature sets;
- clippy with all features;
- the library tests (239);
- the `backend_capabilities`, `provider_transports`, `provider_local`, `boat_provider_contract`, `space_provider_contract` and `runtime_backend_host` tests;
- the `Machines` doctests.

It also compiled each `compile_fail` snippet by hand in a scratch crate. Each one failed only for its intended reason (E0599).

## Round 1

### Design review

The design review found no P1, and no P2 caused by this change.

| Finding | Severity | Decision |
| --- | --- | --- |
| Inherited from #10. The public `BoatMachines::stop` stops any VM in the account, and `Created::restore(.., RollbackNewVm)` followed by `roll_back` deletes any VM in the account. Neither checks ownership. | P2 (inherited) | Documented. Agreed with the Cloud VM thread, which owns that code. Anyone holding a `BoatMachines` already holds the account's API token, so it is not a boundary the SDK can close. The type's rustdoc now says it acts for the whole account, should be handed only to code that owns the account, and that applications should use `Machines` or `OwnedWorkspace`. Nessa still needs `Created::restore` while it keeps its own VM records. When those records move into the SDK, stop and roll_back will go through `OwnedWorkspace` and become `pub(crate)`. |
| `ITarget` pairs bool flags (`stops`, `deletes`, `connects`, `port_policies`) with methods that still default to `Unsupported`, so a new target could set a flag without implementing the method | P3 | Kept. `ITarget` is crate-private, and all five targets are consistent. The adversarial review compared the flags with the behaviour for each provider and ownership. |
| `impl dyn IBackend` covers only `'static` trait objects | P3 | Fixed: `impl<'a> dyn IBackend + 'a`. |
| The doc claim that `MachineUse` "holds at compile time" was too strong, because code that also holds the provider can connect and stop the machine | P3 | Fixed. The doc now says the handle gives no path to stop or delete, and the compiler checks that. |
| `local_capabilities_are_the_accessors_present` re-derives the same formula it checks | P3 | Fixed. The test now also asserts literal expected flags for owned and attached local targets. |
| The `compile_fail` doctests pass on any compile error | P3 | Fixed. They are pinned to `E0599`. |
| `WorkspaceSdk::list()` returns saved capability copies that may be stale | P3 | Fixed. Its rustdoc now says so. |

### Adversarial review

The adversarial review found no P1 or P2. It checked these and found them sound:

- Per-provider behaviour, for local, container, SSH, Boat and SPACE, both owned and attached. The derived flags equal the old hard-coded ones, so existing v4 records are not rewritten on reconnect.
- The moved port-policy check. Nothing gets through that was refused before.
- Every path from `Workspace`, `MachineUse`, `Run` and `BackendRuntimeHost`. None of them reaches stop or delete.
- Saved state. Nothing decides from a stored flag, and refreshing derived flags is an allowed Ready→Ready move.

| Finding | Severity | Decision |
| --- | --- | --- |
| Port refusals lost their hint (Boat used to say to use authenticated hosting) | P3 | Fixed. The message now names the policies the target offers. |
| A container `port: 0` request now gets `Unsupported("ports")` instead of `InvalidInput` | P3 | Kept. The target has no ports at all, so `Unsupported` is the more useful answer. |
| The module doc claimed no capability method refuses for lack of the capability, but a supervisor built without `portability` refuses file browsing | P3 | Fixed. The module doc now says so. |
| `a_saved_ready_descriptor_loads_and_its_flags_decide_nothing` would also pass before this change | P3 | Kept as a regression guard. Its name describes the rule it protects. |
| `Capabilities` has no flag for connections or terminals | P3 | Kept on purpose. Adding flags would change the saved descriptor's shape. Use the live accessors. |
| A connection opened through a `MachineUse` can outlive the use, and the machine can be rested under it | P3 | Documented on `MachineUse`. This was already reachable before, through `backend().connection`. |
| `described()` derives the flags twice | P3 | Kept. It is cheap. |

The round 1 fixes are documentation, a lifetime bound, one error message and stronger assertions. None of them changes behaviour beyond the port refusal text.

No P1 or P2 is open. The gate is accepted once CI passes on both platforms.
