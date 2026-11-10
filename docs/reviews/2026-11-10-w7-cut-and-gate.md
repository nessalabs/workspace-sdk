# W7 review: cut and gate what has no caller

Change: workspace-sdk#11, branch `claude/project-thread-z6wavf`. The branch
does three things:

- it removes managed pipe input, the clean child environment and
  `ManagedEditSession`;
- it puts portability behind the non-default `portability` feature: the
  checkpoint engine, git, sync, recovery, the directory observer, and six file
  browser and tree transfer backend operations;
- it puts the container provider behind the non-default `container` feature.

Two separate agents reviewed it independently: one for design and one
adversarial. The adversarial reviewer ran this compile matrix with
`cargo check --all-targets`:

- default features;
- no default features;
- portability only;
- container only;
- space only;
- testing only;
- all features.

It also ran the cross-build supervisor tests against the real binary.

## Round 1

### Design review

No P1 was found.

| Finding | Severity | Decision |
| --- | --- | --- |
| `ignore` and `flate2` were still required dependencies, although only the checkpoint engine uses them | P2 | Fixed. Both are now optional and turned on by `portability`. `cargo tree --no-default-features` no longer lists them. |
| Deleting the evidence folders for managed editing, pipe input and the clean environment broke links from review records that are kept | P2 | Fixed. The folders are restored as history. |
| `SupervisorOperation::Unknown` and the two-pass decode are more than needed; the gated variants could stay compiled with only the dispatcher arms gated | P3 | Kept. The current design is tested in both builds. It also answers operations that are unknown to the supervisor for any reason, such as a newer client, not only a feature gap. The variant cannot be serialized. |
| Reading a publish `Unsupported` as "nothing published" depends on a rule held by convention | P3 | Kept. The adversarial review traced every transport and the dispatcher, and confirmed that `Unsupported` can only arise before dispatch. |
| The checkpoint domain doc claimed that every value is referenced by saved records | P3 | Fixed. The doc now says the whole module is kept on purpose. |
| The quality gate never checks `container` without `portability` | P3 | Fixed. A container-only clippy run was added. |
| `control_channel.rs` could be folded into `terminal_control.rs` | P3 | Left as is, because it only affects code layout. |
| The state map did not say that the container adapter needs its feature | P3 | Fixed. |

### Adversarial review

No P1 or P2 was found. The reviewer checked these and found them sound:

- the run fingerprint and claim encoding;
- stdin handling for ordinary runs;
- builds where the supervisor and the client have different features;
- what an older deployed supervisor answers;
- a saved state round trip across builds;
- capability flags and docs.

| Finding | Severity | Decision |
| --- | --- | --- |
| The only golden test of the ordinary-run fingerprint was deleted together with the pipe tests | P3 | Fixed. `provider_local::ordinary_claims_keep_their_fingerprint_and_old_piped_claims_never_match` checks two things: the exact salted fingerprint, with no `piped` key; and that an old piped claim gets `Conflict`. |
| A piped run that is still live when an old supervisor is replaced cannot receive input or EOF any more | P3 | Fixed in `docs/release.md`, which now says to cancel piped runs before replacing the supervisor. |
| Crates outside this one cannot override the six portability `IBackend` methods without forwarding the feature | P3 | Fixed in `docs/sdk-guide.md`. |
| `requires_private_transport` was unused in builds without `boat` and `space` (an existing warning) | P3 | Fixed. It is now compiled only for those features. |

## Round 2

A fresh reviewer re-checked the round 1 fixes. Its findings are recorded
below.
