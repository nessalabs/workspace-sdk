# Boat documentation and evidence quality review — 2026-10-12

Reviewer: independent code-quality agent `code_quality_gate`.
Scope: staged documentation, seven evidence renames, redaction inventory keys,
and comments in provider exports/branding. Initial index tree:
`729a5c6bba612feb5e6d2a8927af4faadec003c4`, parent
`3446ac7439aa8e1dee7ca365ec7332550ded90af`. Unstaged PTY integration is excluded.

Applied `coding_standards.md` and the coding skill's review method: correctness
and evidence first, then ownership, dependency direction, cohesion, naming and
simplicity. The Rust edits change comments only. They introduce no abstraction,
state, dependency, allocation, retry, ownership or runtime-behavior change.
There is no SOLID/design-pattern problem to solve by adding machinery here.

## Initial findings

- P2: The architecture's current vendored-client paragraph names `69ca3c9`,
  whereas current public-source evidence names `4d68256`. `vendor/README.md`
  repeats the older current-source claim and old directory. Identify the actual
  published dependency consistently so readers can reproduce the current SDK.
- P2: The edited release validation paragraph calls 93 tests and seven live
  checks current evidence without the later-dependency limitation. Date those
  historical checks and state they do not certify subsequent client revisions.
- P3: The evidence entrypoint links to removed `tests/box_live.rs`; use the
  existing `tests/boat_live.rs`. The evidence patch rename also leaves the
  `vendor/README.md` reference to `box-client-file-write.patch` dangling.
- P3: Architecture still says “Box's” and the SDK guide example variable says
  `explicit_box_api`, inconsistent with the requested displayed Boat naming.

## Evidence inspected

- `git diff --cached` inspected actual documentation, inventory and comment edits.
- Each of the seven renamed files was read from `HEAD:old-path` and
  `:new-path`; every comparison was byte-identical. Raw results, timestamps,
  source/binary digests, resource aliases and blocked-deletion receipts were
  therefore preserved exactly.
- Parsed before/after publication inventories preserve all 134 replacement
  counts under corresponding renamed keys. Every listed file exists.
- Resolved local Markdown links in changed staged documents against tracked
  paths; the sole missing Markdown target is the old test path above. Searched
  tracked Markdown for all seven old evidence filenames to identify the stale
  vendoring reference.
- Inspected the isolated docs-only `scripts/quality-gate.sh` log. It ends with
  “Mechanical checks passed”; real managed-run performance qualification passed
  (one scenario). Mechanical success does not resolve the documentation findings.

## Corrections and re-review

Re-reviewed staged tree `f3b62f5ad4c3d4fbc8b99fe73637b8bcdaf674aa`.
Both P2 findings are resolved: architecture and vendor provenance now identify
`4d68256bb0b6a889633abc567fb09214681874db`; the release results explicitly name
2026-09-08 UTC and exclude certification of later SDK/client revisions. Checked
61 corresponding vendored files against that upstream Git revision; every byte
matched. Both P3 findings are resolved: the test and patch references name the
existing Boat paths, and the remaining displayed names/constructor variable are
corrected. The dated historical source revisions remain in their proper context.

Repeated affected evidence checks after fixes: no missing local Markdown links
in changed documents; seven byte-identical artifact renames; all 134 inventory
replacements retained. `git diff --cached --check` passes. No builds were repeated
for these prose-only fixes; the isolated full mechanical/performance gate's
successful log was inspected above. No additional design or correctness finding.

## Decision

**PASS** for the final documentation/evidence/comment scope. No unresolved
P1/P2 or P3 findings from this review. The independent adversarial decision is
recorded separately. Historical reports remain historical; no new cloud,
SSH/container, native agent login, cleanup completion, or PTY implementation
validation is established by this documentation review. Remote documentation
availability was not exhaustively revalidated by this review.
