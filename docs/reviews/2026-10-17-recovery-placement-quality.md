# Recovery placement: independent quality review

Decision: **PASS** for source commit
`9da3841e3f26fb13f6f0b224afd7ec65f0b7b1b7`.

Reviewer: separate agent `/root/workspace_handoff_quality_review`. The reviewer
inspected the actual source, tests, documentation and final cloud evidence; it
did not implement this increment or run substitute checks.

## Reviewed change

`RecoveryService` coordinates explicit placement using the existing SDK provider
registry, checkpoint transfers and native run claims. A typed `RecoverySession`
retains one canonical checkpoint and pinned attempt. The existing file/memory
repository implementations provide short validated compare-exchange writes and
a separate nonexpiring session effect lease. There is no parallel transfer engine
or expiring distributed ownership implementation. Existing manual handoff remains
unchanged.

The review applied Clean Code, SOLID, cohesion, dependency direction, appropriate
use of concrete collaborators, identity and lifetime rules, and test quality.
No unresolved P1/P2 or lesser actionable finding remains in the accepted scope.

## Findings and disposition

| Finding | Severity | Resolution and re-review |
| --- | --- | --- |
| Publication retry could be mistaken for an idempotent request. | P3 | Public API and guide explicitly say every call captures a new checkpoint; inspect the committed head after an ambiguous response. Closed in frozen source review. |
| Initial cloud gate rejected the version OR pattern. | Mechanical failure | Changed `1 \| 2 \| 3` to equivalent `1..=3`; separate delta review confirmed identical accepted/rejected versions. New full gate passed. |

The separate adversarial review also identified attachment isolation, transient
secret hashing and caller-cancellation ownership issues in the draft. Their fixes
and regressions were included in this review's final frozen scope. See the
[adversarial record](2026-10-17-recovery-placement-adversarial.md).

## Actual validation

[Cloud run 39056115386](https://github.com/nessalabs/workspace-sdk/actions/runs/39056115386)
completed successfully for the exact reviewed SHA on Ubuntu 24.04 and macOS 15.
Both ran `scripts/quality-gate.sh`: formatting; Clippy across all features/targets
with warnings denied; both feature selections' builds/tests; warning-denying docs;
native supervisor build; managed-process and PTY performance qualification.

Each OS and feature selection passed seven new domain tests, seven repository
tests, eleven recovery workflow tests and two real-native caller-cancellation
tests. The ten existing SDK workflow tests also passed. Raw log inspection
confirmed the individual attachment, secret, cancellation and phase-failure
regressions, rather than inferring coverage from a green workflow badge.

Actual performance evidence contains three managed-process and fifteen PTY samples
per OS, with debug assertions enabled, on Linux x86_64 and macOS aarch64. These
hosted samples are qualification, not a statistical baseline or evidence of a
transfer speedup. Full samples, job metadata, log hashes and scope limits are in
[qualification.json](../evidence/recovery-placement-2026-10-17/qualification.json).

The initial run 39055606851 failed Clippy before tests and remains failed evidence.
The new integration tests inject actual receipt write/acknowledgement failures and
reopen real storage. No new production mutation probes were performed; earlier
manual-handoff mutation probes are not attributed to this feature.

## Remaining limits

Acceptance covers explicit caller-driven checkpoint placement. Availability and
Boat placement are simulated using isolated real native workspaces. Physical host
loss, offsite replication, automatic health detection, expiring distributed
ownership, native agent conversation conversion and live remote failover are not
qualified. Quiescence and external-effect fencing are application contracts.
Runtime/process loss may leave retained artifacts or transfer staging without an
aggregate garbage collector. Full archive transfer and the existing 32 MiB JSON
authority bound remain unchanged. These limits are public in the SDK guide.
