# Git worktree discovery: independent adversarial review

Decision: **FINAL PASS** for
`baac6392cad4af28635ba9c2f4fe99667a729b1c`.

Independent reviewer: `/root/workspace_handoff_adversarial_review`, separate from
implementation and the quality reviewer. Review examined malformed/partial output,
truncation, inherited Git redirects, bounds, stale observations, ownership claims,
native fixture safety and misleading provider evidence.

No unresolved actionable findings remain. A pre-gate static test-hygiene finding
was fixed: fixture-local hook/signing configuration now protects SDK-executed Git
commands from unrelated host-global checkout hooks. This changes only disposable
test repositories; production retains normal host Git configuration and trust checks.

The service requests one fixed argv command and an execution deadline. It rejects
failed, absent, timed-out and truncated results before parsing. Strict framing,
duplicate detection, exclusive HEAD state and size limits prevent partial accepted
inventories. Empty lock/prune reasons are preserved. No discovery value grants
attachment, deletion or resource ownership, and there is no automatic retry.

## Evidence inspected

The reviewer verified both exact checkout SHAs and successful full gates in
[run 39072061545](https://github.com/nessalabs/workspace-sdk/actions/runs/39072061545).
Each OS and feature selection passed eight Git unit tests and four native tests.
The latter discover real linked paths and metadata, run explicit Git creation
through an anchor, compose existing attachments and execute independently without
changing the primary checkout. Missing Git and non-repository failures remain errors.

The gate includes formatting, Clippy, docs, builds/tests and managed/PTY performance
qualification. Performance records match raw logs: three managed and fifteen PTY
debug samples per platform. These are qualification samples, not statistical
performance guarantees. See
[qualification.json](../evidence/git-worktree-discovery-2026-10-17/qualification.json)
for actual samples, hashes, counts and limits.

## Remaining limits

Git observations can race filesystem changes. Host paths are locators tied to the
anchor's execution route, not durable cross-host identities or authorization.
Metadata must be unambiguous UTF-8; unsupported fields cause failure. Acceptance
bounds do not replace backend capture/allocation limits. Caller-drop and transport
behavior remain the backend's responsibility.

Native evidence is Local only. No live SSH/Boat, Windows, SDK-owned Git worktree
creation/removal, Git mutation retry guarantees or Git-aware checkpoint migration
are established. No new live resource was created for this qualification.
