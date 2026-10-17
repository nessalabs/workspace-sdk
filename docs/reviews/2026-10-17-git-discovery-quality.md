# Git worktree discovery: independent quality review

Decision: **FINAL PASS** for
`baac6392cad4af28635ba9c2f4fe99667a729b1c`.

Independent reviewer: `/root/boat_cleanup_quality`. The reviewer inspected the
actual Git module, public export, native tests, guide, exact cloud checkout SHAs,
individual test logs and performance samples. Review applied Clean Code, SOLID,
cohesion, dependency direction, bounded parsing and identity/ownership rules.

The concrete discovery service composes `Workspace::exec`; it introduces no new
provider contract, registry, persistent identity or lifecycle ownership. Its
inventory carries anchor provenance while target paths remain observations.
The parser checks complete NUL framing, duplicate and unknown fields, mutually
exclusive HEAD states, OIDs/references and all documented acceptance bounds.
Command construction uses argv, clears redirects/inline overrides and retains
host configuration and `safe.directory`. Result interpretation does not echo Git stderr.

## Finding and resolution

One static P2 test-hygiene finding was resolved before execution. The fixture Git
helper disabled hooks per command, but the later SDK-executed `worktree add` could
still inherit host-global checkout hooks. Fixture repositories now persist local
`core.hooksPath=/dev/null` and `commit.gpgSign=false` immediately after initialization.
Both independent reviewers inspected this change. Production discovery policy is
unchanged, and no user hook was deliberately executed to reproduce it.

No unresolved P1/P2 or lesser actionable finding remains in this accepted scope.

## Actual checks

[Run 39072061545](https://github.com/nessalabs/workspace-sdk/actions/runs/39072061545)
passed the full `scripts/quality-gate.sh` on Ubuntu 24.04 and macOS 15 for the exact
reviewed source. Each OS ran eight Git unit tests and four real Git/native tests
under both feature selections. The reviewer checked all sixteen unit and eight
native executions per OS rather than inferring coverage from the workflow badge.

The full gate includes formatting, warning-denying Clippy/docs, both feature
selections' builds/tests, supervisor build and managed/PTY performance qualification.
Each platform produced three managed and fifteen PTY debug samples with bounded
retained output. These are qualification samples, not a statistical baseline or
Git inventory throughput benchmark. Samples, hashes and job metadata are in
[qualification.json](../evidence/git-worktree-discovery-2026-10-17/qualification.json).
No old-source red execution or new mutation probes are claimed.

## Scope limits

Native evidence covers Local execution and explicit attachments, not live SSH,
Boat or Windows. Inventory can become stale immediately and grants no ownership.
The 256 KiB limit applies after backend capture; UTF-8 replacement characters are
refused. Existing backend deadline, capture and cancellation responsibilities remain.
Explicit Git creation in the native test is a caller command, not a new SDK Git
lifecycle or retry guarantee. No managed worktree deletion or registry is added.
