# Boat evidence documentation: independent adversarial review

Reviewer: retained_eof_adversarial. Scope is the staged evidence filename/prose
update and two comment-only provider files; unfinished PTY code is excluded.
Reviewed against parent 3446ac7439aa8e1dee7ca365ec7332550ded90af using the actual
index diff, independently of the implementation and code-quality review. No
builds or live requests were run by this reviewer.

## Evidence integrity

Compared each HEAD:old-path blob with the corresponding staged new-path blob.
All seven renamed JSON/patch artifacts are byte-identical. Dates, measured
results, binary/source digests, private resource aliases and blocked-deletion
outcomes are therefore unchanged. The renamed redaction-inventory keys still
sum to 134 replacements, and every listed target file exists. No private
original is added to the publication tree by these changes.

The prose keeps the historical probe dates and distinguishes tested corrected
builds from unpatched revisions. It continues to say deletion is blocked and
completion unconfirmed, that no model calls/real credential transfers were
performed, and that current client checks are offline rather than new cloud
validation. The code diff changes comments only, introducing no process,
credential, target, lock, retry or allocation behavior.

## Findings

- P2: The current integration paragraph in docs/architecture.md still identifies
  69ca3c9 as the vendored source, contrary to current 4d68256 provenance. Current
  source claims must agree; historical dates/revision hashes should remain
  historical. The separate quality reviewer also identified the stale
  vendor/README.md claim.
- P2: docs/release.md describes old 93-test/seven-live-check evidence as current
  without the subsequent-dependency qualification retained in the evidence
  entrypoint. The renamed wording must not imply those live probes certify the
  present dependency. This overlaps the separate quality review finding.
- P3: The evidence entrypoint still links to removed tests/box_live.rs. Local
  Markdown link checking found that target missing; use tests/boat_live.rs.
  Remaining Box/Ascii Boat prose should follow the requested naming consistently.

## Mechanical evidence and decision

Inspected work/pty-integration/docs-only/gate.log. The isolated, non-PTY source
passes scripts/quality-gate.sh through its explicit real-local performance
qualification and final mechanical-success marker. This does not resolve the
documentation findings or validate unfinished PTY work.

Decision: pending provenance/validation-scope corrections and re-review.
No new runtime-risk finding applies to this documentation-only change. The
two unresolved P2 documentation findings block scoped acceptance; no live
service, cleanup completion, agent authentication or PTY claim is added.

## Corrections and final re-review

Re-read the updated staged documents. Architecture and vendor/README.md now
identify current vendored source 4d68256 and the correct vendor/sandbox-client
directory. The release section dates the 93-test/seven-live-check result to
2026-09-08 UTC and explicitly says it does not live-certify later client or SDK
revisions. Both P2 findings are closed. The native test link now resolves to
tests/boat_live.rs, the renamed patch reference is corrected, and relative
Markdown link validation finds no missing targets in changed staged documents.
The P3 link/naming finding is closed.

Raw historical evidence remains unchanged under the requested display naming.
The inspected isolated gate is complete and successful; later corrections are
prose/provenance/link changes only. They do not change the qualified executable
or performance workload. Final scoped adversarial decision: PASS, no unresolved
P1/P2 findings. This accepts only the documentation/evidence update and does not
accept or certify the separate in-progress PTY implementation.
