# Evidence label normalization quality review — 2026-10-12

Independent reviewer: `code_quality_gate`. Scope: the later unstaged changes in
`docs/evidence`, including JSON keys/values/path labels, the historical file-write
patch, and the normalization disclosure. This supersedes the earlier byte-identity
review only for these newly authorized label edits; historical result values must
remain unchanged. No production source or evidence was edited by this reviewer.

## Findings

P2, resolved after re-review: the patch's unchanged context was mechanically rewritten to
`/boates/{}/files` and `validate_boat_id`. That endpoint is invented, and the
advertised historical correction no longer applies to its recorded source.
Retain only the actual POST-to-PUT line in a zero-context unified diff at the
historical line number. This removes old display naming without fabricating an
API or changing the correction. The public normalization disclosure does not make
invented executable patch context reliable evidence.

## Evidence checks

A recursive before/after comparison of all 13 changed JSON objects confirmed
that they differ only by the requested provider label transformations. Object
keys remain unique. Numbers, booleans, nulls, timestamps, result statuses,
measurements, hash strings, and private alias multiplicities remain unchanged.
All 15 JSON files in the evidence directory parse with duplicate-key rejection.
All relative Markdown links in the evidence README resolve. The disclosure now
states that labels/paths are normalized and that archived digests describe
historical sources, not the current source tree. No new live result or completed
cleanup is claimed.

## Correction and re-review

The patch now contains only the original POST-to-PUT changed line, at line 429
as established by the original hunk's three preceding context lines starting at
426. The invalid endpoint/helper context is absent. The evidence table explicitly
calls this a zero-context excerpt. Its added and removed statements remain exact
matches of the original correction. The P2 is closed; no unresolved finding.

## Decision

**PASS** for the focused evidence-label normalization scope. This is separate
from the ongoing PTY implementation gate. JSON/hash/alias and link checks above
remain applicable; no executable code changed in this evidence correction.

Final PTY mechanical evidence is pending: the initial `final-gate.log` ended in
a Socket-to-UnixStream compilation error; the implementing agent corrected the
conversion and `final-gate02.log` is running. No tests/builds were launched by
this reviewer.
