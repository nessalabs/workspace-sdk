# Boat migration and public source: adversarial review

Reviewer: retained_eof_adversarial, independent of implementation and the
code-quality reviewer. Reviewed the current source against the pre-migration
publication candidate, coding_standards.md, the Boat vendor snapshot, public
identity/schema tests, publication evidence changes and the completed mechanical
gate. No builds, live requests or cloud mutations were run by this reviewer.

## Decision

PASS for the reviewed Boat adapter migration and evidence-preservation scope.
No unresolved P1/P2 findings were found. This is not a current Boat cloud
conformance claim, a PTY integration approval or a substitute for the separate
publication privacy/allowlist review.

## Runtime and identity checks

The implementation moves from providers/ascii_box to providers/sandbox and
uses the current vendored client through a neutral dependency name. Actual
runtime changes are the Boat resource/envelope names and model fields. Managed
backend, supervisor, private-file transport, ownership gates, preparation,
cancellation and synchronization control flow are unchanged.

The default registration ID remains ascii-box. Boat and legacy public names
refer to the same implementation; no second provider identity silently steals
old references. Explicit with_id registrations remain isolated. Existing
serialized box_id/box_type options remain canonical; sandbox aliases decode
and duplicate old/new spellings are rejected, including conflicting/null forms.
The Rust field rename is documented as a source change. Independent public-API
tests verify saved reference identity, rejected cross-provider connect,
registration conflicts, attached capabilities, option round trips and defaults.

The adapter accepts an explicitly constructed client and does not discover API
credentials from the environment. The vendored request layer still retries GET
only. Commands, creation, deletion and private request-file writes do not gain
automatic mutation retries. The synchronous command route is used without
NDJSON; the previously reviewed client rejects unsupported synchronous streaming
before submission. Minimal cancelled sandbox responses now decode and terminate
readiness waits through the client. State observation remains conservative.

Sensitive supervisor launch/write payloads still use the private file API,
not command text. UUID-scoped cleanup does not replace the primary result.
Attached targets cannot be resumed, stopped or deleted by the SDK. Reconnected
owned targets still need ownership evidence before lifecycle mutation; deletion
polling still checks the operation target. Authenticated hosting explicitly
requests public=false and requires protected HTTPS in the response. Existing
real HTTP boundary tests for these paths pass under the new Boat routes.

All 61 vendored files byte-match the supplied client source archive with no
extra files. Provenance names Boat client commit
070431ae4eb5fb20dfea551fca292fbc008cc47c. The included independent client review
records cover the current wire-contract, unsafe-stream and loopback-validation
fixes; workspace code does not modify those mechanisms.

## Publication evidence checks

Compared all 14 privately retained original JSON reports with their public
counterparts without emitting original values. Ignoring the explicit added
publication_redactions metadata, there are exactly 134 changed string leaves
across 12 reports and no changes to object shape, array length, numeric values,
booleans or nulls. Every replacement is a private alias. Changed fields are
resource/deletion identifiers and machine/binary/root/control paths; result
statuses, counts and measurements are preserved. The inventory matches those
changes. Historical build digests and live outcomes are explicitly qualified as
historical, not evidence for the renamed current source or completed cleanup.

The root ignore policy excludes work, target, dist, environment files and common
private-key containers. Private originals are under ignored work. A final
allowlisted publication tree and separate privacy review remain necessary:
ignore patterns alone do not certify arbitrary manually selected files.

## Commands and evidence inspected

Read-only directory/source comparisons, archive byte comparisons and structured
JSON comparisons were used for this review. Implementation-owned evidence:

- work/boat-migration/contracts-default.log and contracts-legacy.log: four
  independent migration contracts pass in both feature configurations.
- work/boat-migration/gate02.log: scripts/quality-gate.sh completes successfully,
  including formatting, locked all-feature/all-target Clippy, all-feature and
  no-default-feature tests, warning-denied rustdoc, supervisor build and explicit
  ignored local performance qualification.
- The gate includes 62 all-feature unit tests, four migration contracts, existing
  workflow/recovery/security/runtime suites, and 58 no-default-feature unit
  tests. Ignored cloud tests are not counted as service validation.

The current performance report records macOS aarch64, debug assertions enabled,
three sequential samples under concurrent stdout/stderr load and a combined
32 KiB retention budget. Launch acknowledgements were 41.777291, 26.223334 and
27.141584 ms; cancellation was 74.010500, 75.093209 and 78.488375 ms. Each stream
advanced at least 262144 bytes and retained exactly 16384 bytes. The test asserts
single execution after reconnect/retry, bounded output and retained tail recovery,
and persisted cancellation. These are scoped real-local process checks, not a
statistical regression baseline, RSS/CPU measurement or Boat transport benchmark.

## Remaining limitations

No new Boat live probes, agent authentication, SSH/container conformance or PTY
integration were performed. The publication and rename preserve existing
behavior; they do not newly qualify cancellation during private-file delivery,
cloud resource cleanup after unknown delivery or all provider lifecycle states.
The current checks and limits are stated explicitly rather than treating an
offline mock, ignored test or historical live result as a present cloud pass.

## Current-only policy: superseding re-review

The user explicitly requested removal of backward compatibility after the first
review. This section supersedes the earlier acceptance of the ascii-box feature,
provider default, public aliases and serialized Box option fields. They are
removed. The current default provider ID is boat; serialization uses sandbox_id
and sandbox_type exclusively, with deny_unknown_fields. Old-only fields, mixtures
of old/current fields and duplicate current fields fail decoding rather than
silently choosing an attachment target or creating a replacement resource.

The unchanged managed locator decoder requires an exact provider ID before
decoding a locator. The new default cannot reinterpret an old ascii-box
reference. Explicit caller-selected registry IDs remain possible through the
ordinary with_id API; there is no automatic migration or compatibility mapping.
Independent tests still assert clone isolation, rejected cross-registration
connections, exact saved references, attached destroy restrictions and duplicate
registration rejection. This is an intentional breaking policy, documented in
README, rather than a continuation of the prior compatibility guarantee.

The refreshed vendor source is Boat client
4d68256bb0b6a889633abc567fb09214681874db. All 61 files byte-match the supplied
client-current-only archive with no extras. Its separately reviewed cleanup
rejects obsolete handoff identity fields and narrows 409 retry eligibility to
boat_starting; journal state/CAS and mutation retry policy are unchanged.

Inspected work/boat-migration/current-only-red.log (one contract passed, four
failed before the change), current-only-green.log (all five contracts pass),
and the completed current-only-gate.log. The full scripts/quality-gate.sh run
passes after the updated vendor and current-only SDK changes, including both
feature configurations, warning-denied Clippy/docs, supervisor build and explicit
real-local performance qualification. No reviewer builds were run.

The final macOS aarch64 debug-enabled performance samples report launch
29.668541, 36.962000 and 35.919750 ms, and cancellation 76.243166, 75.371708 and
74.549083 ms. Each stream advances at least 294912 bytes and retains exactly
16384 bytes. The workload, correctness assertions and previously stated limits
remain unchanged; these timings are not a statistical regression baseline.

Current-only final adversarial decision: PASS, no unresolved P1/P2 findings in
the reviewed change. Old source names and stored records are intentionally
unsupported. Publication privacy review remains separate; no current cloud or
PTY validation is inferred from this decision.
