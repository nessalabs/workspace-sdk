# Boat migration independent adversarial review

Reviewer: retained_eof_adversarial. Reviewed the working change against
69ca3c9 in box-client, including production code, migration fixtures, independent
wire tests, CI, retained handoff persistence and completed mechanical logs.
The reviewer authored tests/boat_boundaries.rs and did not change production
code or run builds concurrently with the implementation agent.

## Findings and fixes

1. **P2, environment scope:** the published Boat contract uses `?org=` for
   environment operations. Sending only the wallet header can affect personal
   resources instead of the intended team environment. Fixed centrally at
   request construction for `/environments` and its descendants. Independent
   HTTP tests exercise listing and nested variable mutation with reserved
   characters in the organization, and prove sandbox calls retain the header.
   Re-review found the routing and percent encoding correct; the initial five
   boundary tests pass in work/boat-final-tests.log.
2. **P2, insecure base URL validation:** string-prefix loopback recognition
   accepted remote `127.evil.example`, and public configuration fields bypassed
   builder validation. Fixed by matching parsed URL host types and validating
   again in Api::new. Independent regressions reject remote numeric-prefix
   domains, ordinary remote IPv4/IPv6 and constructor mutation, while retaining
   real loopback forms and HTTPS. work/boat-boundaries-red.log captures the
   constructor bypass; the final five-test boundary run is green. Closed.
3. **P2, misleading scope contract:** with_org(None) was documented as personal
   scope although it clears explicit selection; sandbox/limits then use server
   wallet defaults. Rustdoc now distinguishes cleared selection from explicit
   `Some("personal")`. Added a public-API regression checking omitted headers
   and queries for None, personal header/query placement, and clone isolation.
   Documentation re-review passes; the new test passes in both
   work/boat-stream-green.log and work/boat-gate-tests02.log. Closed.
4. **P2, command stream mode:** newly exposed stream=true switches Boat to
   NDJSON, but command_raw still buffers and decodes a single JSON CommandResult.
   A command can execute and then appear to fail locally. The generated fixture
   concealed this by pairing stream=true with an ordinary JSON response.
   Published OpenAPI command documentation and SDK CommandRequest describe the
   streaming response. Added an independent actual-NDJSON mock asserting both
   JSON command methods reject streaming before any remote execution. This is
   reproduced failure in work/boat-stream-red.log and
   work/boat-command-stream-red.log. Fixed with a command_raw guard before
   request construction; command delegates to that guarded path. Detached
   requests retain the documented behavior of ignoring stream. Canonical
   fixtures now explicitly use stream=false through actual SDK builders.
   Re-review inspected the guard, SDK contract, fixture generator and tests:
   seven boundary and four command-mode tests pass, including no POST for
   synchronous stream=true, one POST for detached mode, and ordinary JSON
   completion for stream=false. Closed after affected and full checks.

## Other adversarial checks

The inventory preserves all 59 previously supported operations under the current
documented names. It does not imply coverage of every newer SDK operation.
Mutation retries remain disabled, including command requests whose ambiguous
502 failures may mean execution already started. GET retry budgets, response
caps, cancellation, page draining and redacted transport diagnostics retain
their existing ownership and control flow. The cancelled state now ends
readiness waits; independent wire tests cover the minimal cancelled response.
Explicit nullable request options retain omission/null/value distinctions.

The rename preserves bx_ identifiers, legacy deletion/webhook wire headers and
schema-v1 handoff JSON keys. The full persisted receipt regression compares
exact JSON before and after serialization, preventing an accidental journal
schema migration. Handoff lock/CAS/reconciliation behavior is unchanged.

## Evidence and limitations

Inspected work/boat-final-tests.log (all enabled targets pass),
boat-final-clippy.log, boat-final-doc.log (12 pass), boat-final-rustdoc.log,
boat-final-sdk.log (59 operation contracts and 13 runtime contracts), and
boat-final-performance.log. Those logs precede the added scope and command-mode
regressions. Subsequently inspected completed work/boat-gate-tests02.log,
boat-gate-clippy02.log, boat-gate-doc02.log (12 pass), boat-gate-rustdoc02.log,
boat-gate-sdk02.log (59 operations and 13 runtime fixtures), and
boat-gate-performance02.log. All enabled tests/checks pass after the fixes.

The performance test performs 384 sequential loopback reads through one client
in three samples of 128 and checks responses and request counts. Recorded
final samples: 19.362417, 16.255625 and 13.804875 ms, macOS aarch64. This is bounded
functional/load smoke evidence; it measures neither socket reuse nor RSS and
does not establish a statistical regression baseline or cloud performance.
Ignored live tests and historical Box smoke evidence do not establish current
Boat service behavior. No credentials or cloud mutations were used by this
reviewer. docs/testing.md now explicitly qualifies the historical live evidence.

Final scoped verdict: no unresolved P1/P2 findings in the reviewed migration.
Adversarial acceptance passes for the supported offline-verified operation
surface with documented NDJSON and live/performance limitations. This does not
extend to unimplemented newer endpoints or certify current cloud behavior.

## Current-only cleanup re-review

The user subsequently explicitly removed the backward-compatibility requirement.
This supersedes the earlier receipt-key preservation conclusion above. Reviewed
the small follow-up diff against 070431a: HandoffPlan now writes only
source_sandbox_id/target_sandbox_id, requires those fields on read, and rejects
unknown fields. Old-only, mixed old/new and extra obsolete fields cannot silently
select another identity. Existing durable files that use obsolete names fail
with Corrupt rather than being treated as missing or overwritten; this is an
intentional breaking change. Journal locking, CAS, phase transitions and unknown
delivery handling remain unchanged.

409 retry classification now recognizes only the verified boat_starting code;
obsolete box_starting/box_securing no longer trigger retry. This narrows retry
eligibility and does not enable mutation replay. Verified current service
identifiers and required deletion/webhook protocol names remain intact.

Inspected work/boat-current-only-red.log (six boundary tests pass, the new receipt
contract fails), followed by boat-current-only-tests.log (166 enabled tests pass,
including all seven boundaries), boat-current-only-clippy.log,
boat-current-only-doc.log (12 pass), boat-current-only-sdk.log (59 operation and
13 runtime comparisons) and boat-current-only-performance.log. The scoped
loopback samples are 18.032458, 17.453584 and 13.627917 ms with unchanged workload
and limitations. No reviewer builds were run.

Current-only scoped verdict: PASS, with no unresolved P1/P2 findings. Obsolete
receipt files are deliberately incompatible, as requested; this verdict does
not reinstate the prior compatibility guarantee or broaden live coverage.
