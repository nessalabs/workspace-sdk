# Boat publication migration: independent quality and performance review

Reviewer: performance_gate. Scope: current workspace-sdk against the saved
work/publication-candidate baseline, including provider adapter changes, public
aliases, durable identifiers, vendored client provenance, migration tests and CI.
Read coding_standards.md and AGENTS.md. No reviewer builds or benchmark reruns.

## Decision

PASS for the scoped change, with no unresolved P1/P2 quality finding. This is
an independent Clean Code/SOLID, dependency-direction, cohesion and performance
review; the separate adversarial review remains required for final acceptance.

## Architecture and identity

Implementation is owned by providers/sandbox with SandboxProvider and
SandboxOpenOptions. Boat aliases expose the product terminology without
duplicating provider state or behavior. The external client is imported under
the neutral sandbox_client dependency alias; its public Boat client re-export
and legacy alias expose the same pinned types. The existing IProvider/IBackend
boundary remains intact, and domain changes are documentation only. No vendor
DTO or transport dependency is introduced into domain ownership.

The adapter retains its existing responsibilities: target selection and
creation, lifecycle ownership validation, native supervisor dispatch, deletion
translation and provider error conversion. It reuses the client's transport
and typed API rather than implementing request handling again. Changes from
the saved adapter are the neutral names, current sandbox DTO envelope and
methods, explicit omission of the new localhost port option, and display text.
Launch/save ordering, command execution, lifecycle checks, state translation
and synchronization are unchanged. No new per-byte work, lock, allocation loop
or retry path is introduced by this migration. No new design pattern is needed.

Branding owns a display name separately from DEFAULT_PROVIDER_ID. The default
registration remains ascii-box so existing references keep their identity;
with_id continues to isolate separately configured registrations. Rust option
fields are sandbox_id/sandbox_type, while serde retains canonical box_id/box_type
and accepts the new spellings as aliases. deny_unknown_fields still catches
misspellings. Tests reject duplicate old/new fields in both orders, including
matching values, conflicting values and null, so field order cannot select
a different target. Explicit null TTL survives as no auto-stop, while omitted
TTL remains one hour. This is serialized compatibility; renamed Rust fields
are still a source-level change in the pre-publication API.

## Dependency and verification evidence

Independently compared every one of the 61 vendored files against Boat client
commit 070431a: all bytes match. Its src, tests and scripts populations are
complete. No local dependency patch was introduced. That revision includes the
reviewed host validation, query organization scope, nullable/cancelled models
and pre-dispatch rejection of unsupported synchronous command streaming.
Workspace adapter command helpers continue to use ordinary JSON command mode.

contracts-default.log and contracts-legacy.log each execute and pass all four
public migration tests. These prove alias identity, persisted option round
trips, duplicate rejection, custom-provider isolation, wrong-provider rejection
and reconnect registration behavior; they do not establish live Boat access.
The completed gate02.log ends with the mechanical pass marker. Across its
all-features, no-default-features, doc and explicit performance phases it reports
221 passed test executions, zero failed and three ignored phase entries. The
ignored performance test is subsequently invoked explicitly and passes.
Formatting, strict Clippy and rustdoc also finish successfully. CI runs the
same quality script on Ubuntu and macOS and explicitly adds the legacy-feature
migration contract invocation. Hosted CI execution is not claimed by this
local review.

## Scoped performance

Reviewed the unchanged real-local-provider harness and current JSON artifact.
It uses the native supervisor with a compiled producer, three sequential
samples, 128 KiB minimum observed output per stream, a 32 KiB total retention
budget, 4 KiB read pages and 10 ms polling. Assertions verify exactly-once launch
through reconnect/retry, retention-gap signaling, bounded pages, active-load
cancellation, exact retained byte content and stable terminal cursors. The
report is written only after all assertions pass, and an old report is removed
before a run. This is stronger evidence than merely collecting timestamps.

macOS/aarch64 debug-build samples from gate02:

| Sample | Launch ms | First output ms | Reconnect ms | Retry ms | Loaded read ms | Cancel ms | Final offset per stream |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 41.777 | 264.602 | 0.034 | 2.033 | 1.880 | 74.011 | 274432 |
| 2 | 26.223 | 70.740 | 0.059 | 3.318 | 2.626 | 75.093 | 262144 |
| 3 | 27.142 | 67.522 | 0.045 | 2.874 | 2.339 | 78.488 | 274432 |

Every sample retains exactly 16,384 bytes per stream and passes cancellation
and replay assertions. These are scoped measurements, not a speed threshold
or statistical regression baseline. No Boat WAN, live provider, SSH/container,
PTY, terminal rendering, RSS or descendant-cancellation qualification follows
from this local harness. Historical live evidence predates this migration and
must remain labeled accordingly.

## Reviewed file identities

- `Cargo.toml`: `467ac9e1b8f6d9e0dfd2e7d62f2306f9249ae328b81f214365e599f7ca6a0517`

- `src/providers/mod.rs`: `b73b72e5b399e9330c3ae4d882960c84ab8c6b55d0da22c7970a9a3f8d744236`

- `src/providers/sandbox/branding.rs`: `854b29c87e85467a1597f84d230ce66bd3721766e76ac6ffdc3c038e3a9f25fd`

- `src/providers/sandbox/configuration.rs`: `930a955580a7796b02db12dc9ad216bdf1ce6ec3af67f13901b98c898c51521f`

- `src/providers/sandbox/provider.rs`: `3c8307f18b2ab512feb81dd4e20076150a94d75a89433227e2f7a94552649306`

- `src/providers/sandbox/target.rs`: `0f6878c3a4a701b58e3e8ad08d2278db4bf705149694c0ca4341fe97cc8ca3ae`

- `tests/boat_provider_contract.rs`: `3fb6e9a9818ee617696a24543a9c01efaee3491e5a64fe54fce9e1f0ab5b1981`

- `.github/workflows/ci.yml`: `84731715f4ce73cbc390419f427c7e4a4f26f0d2511435496cd29673be096ea3`

- `scripts/quality-gate.sh`: `7cff90ed9386037bb3aeaedac01cf8ee30e0f80543e9fb429adca046e3c30334`

- `tests/runtime_performance.rs`: `03f79f0477250e0f947531ca968429d8a8c8b129241bbbb6866efa64c0e8576a`

- `target/runtime-performance.json`: `1c2beaef89358fa5677bfd98dcadd7fe7086a89dba99093272a2b1a6087a9ac8`

- `work/boat-migration/gate02.log`: `bfff5b65989003c78effe182e2be904a04033bca10fcf7df60caead3c0bc7668`

## Current-only scope after explicit compatibility removal

The user explicitly requires no backward compatibility. This follow-up
supersedes the earlier compatibility verdict: default provider identity is now
boat; ascii-box feature and AsciiBoxProvider/BoxOpenOptions/ascii_box_client
exports are removed; serialized options use sandbox_id and sandbox_type only.
There is no legacy serde rename or alias. deny_unknown_fields rejects obsolete
keys rather than silently ignoring a target. This deliberate pre-publication
identity/schema break does not automatically reinterpret saved records.
Explicit with_id remains an ordinary caller-selected registration capability,
not an implicit legacy fallback.

Reviewed current tests: the current schema round-trips through ProviderOptions;
obsolete fields fail alone and alongside current fields in both orders; duplicate
current identity/type keys reject identical, conflicting and null values in
both orders; explicit provider registrations remain isolated. The retained
current-only-red log has one pass and four expected failures against the former
compatibility implementation. current-only-green executes and passes all five.

All 61 current vendored client files independently match commit 4d68256 byte for
byte. That revision removes obsolete Handoff identity wire fields, rejects
unknown plan keys and drops legacy retry codes, as independently reviewed in
the client repository. No workspace-side dependency patch is present. CI now
runs the regular current-feature gate on both platforms without advertising a
removed compatibility feature. The neutral provider structure, current Boat
public aliases, domain dependency direction and lifecycle ownership are intact.

Current-only source quality review passes with no P1/P2 finding. Full current
mechanical/performance evidence is still being collected; final gate status
will be recorded below after completion.

### Final current-only gate re-review

The completed current-only-gate.log ends with the mechanical pass marker and
reports 222 passed test executions across phases, zero failed and three ignored
phase entries. The managed performance test is explicitly invoked and passes.
The current JSON report matches the report printed in that log exactly. All
format, strict Clippy, feature-matrix and rustdoc checks complete successfully.

Current macOS/aarch64 debug-build samples:

| Sample | Launch ms | First output ms | Reconnect ms | Retry ms | Loaded read ms | Cancel ms | Final offset per stream |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 29.669 | 243.338 | 0.037 | 2.167 | 1.929 | 76.243 | 323584 |
| 2 | 36.962 | 91.462 | 0.040 | 2.363 | 2.179 | 75.372 | 294912 |
| 3 | 35.920 | 88.175 | 0.042 | 2.156 | 2.537 | 74.549 | 356352 |

All samples retain exactly 16,384 bytes per stream and pass the unchanged
real managed-workload assertions. These measurements replace the earlier
compatibility revision's samples as evidence for current-only acceptance.
They carry the same local/debug scope and do not establish cloud behavior,
RSS, PTY qualification or a statistical performance baseline.

Final current-only quality and scoped performance verdict: PASS, no unresolved
P1/P2 findings. No reviewer builds or benchmark reruns were performed.

Current reviewed identities:

- `Cargo.toml`: `972e1ba27fdf45c9d929bf3fe76e61901b817ff152fc577172665a82a3ff6077`

- `src/lib.rs`: `ec98d7011a9b6c59f45713dc7cde48afae95e53df9f46615171d181773ee0db8`

- `src/providers/mod.rs`: `a49c934cd3cd2e5dcfb650380135f165a7e0169b14c10dc73554b38b9266c093`

- `src/providers/sandbox/branding.rs`: `f4a50f6a07665fb8b02a1ed2d96f5a423162386e52ce268cf22a70d02ec1f384`

- `src/providers/sandbox/configuration.rs`: `6989e3e213cb438ddabfc9165801e816b567e4299a190f06fb25b21d6439eecd`

- `tests/boat_provider_contract.rs`: `4f8ae7ceef4cf123878f43398c7ac4e5bca9357ec9aacf9539c525f5654f24d0`

- `.github/workflows/ci.yml`: `ab6318b39671b41808e07946d207d94551fee266a370b4401c62a0daae9dc75f`

- `target/runtime-performance.json`: `50271acd6a6d43e07cb3b93dd81d08f4ddcabce31409c92360497c7fab9dcaff`

- `work/boat-migration/current-only-gate.log`: `49e8abfb9fc596f1ce5d334b6791e74d18bc240ab0f0a670b78f5826703abe8c`
