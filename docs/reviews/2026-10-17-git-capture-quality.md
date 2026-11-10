# Source Git discovery quality review

- Reviewer: independent quality/design agent `/root/boat_cleanup_quality`.
- Exact source: `db6e2fa7141503d6f91c079759a1c647f3f2afba`.
- Scope: source tracked-file discovery in `capture.rs`, five linked-repository integration cases, two private marker tests, and the SDK guide clarification.
- Final decision: **PASS**. No unresolved P1/P2 quality/design findings in this scope.

## Finding, fix, and re-review

| Severity | Finding | Final disposition |
| --- | --- | --- |
| P2 | Every failed `rev-parse --is-inside-work-tree` probe became an empty tracked set. Broken linked common metadata could therefore turn tracked files matching ignore rules into omitted untracked files while capture/export succeeded. | Fixed: successful output must be exactly `true\n` or `false\n`. Failure or exact false examines root/ancestor `.git` entries using no-follow metadata. Any entry, including a dangling marker, refuses incomplete selection; non-NotFound metadata errors propagate. Only no-marker context retains legacy plain scanning. Source re-review and exact-source gate pass. |

The change conservatively recognizes observed Git context instead of inventing a safe fallback from Git exit status or English stderr. Successful worktree probing retains the existing `ls-files` path and its error handling. No pointer parsing, public API, new interface, registry, or competing Git environment policy was introduced. Existing checkpoint commands already clear inherited Git environment overrides. The marker helper walks ancestry directly without accumulating directories, and all current callers supply canonical checked roots.

Source review explicitly preserved legacy behavior for bare repositories without `.git` markers. It also required the guide to distinguish quiescent metadata inspection from atomic repository-identity binding. Those clarifications are in the final source. The proposed scanner performance change was deferred and is outside this increment.

## Test assessment

Real disposable repositories establish that the payload is both tracked and ignored before linking. The healthy case proves root and nested tracked content is selected/restored, ordinary ignored content and Git metadata are excluded, and source pointer, payload and Git metadata bytes remain unchanged.

Failure fixtures rename only their exclusively owned common Git metadata, leaving the linked pointer intact. Capture and local tree archive export must refuse; a subdirectory case requires finding the broken marker on an ancestor. A fifth case directly invokes `export_remote_tree` and verifies that the archive was not published. That case exercises the real target-side filesystem helper, not provider transport or a live remote host. Normal SDK control-directory setup is permitted; the test does not claim the entire project directory remains byte-for-byte untouched.

The two private tests cover no marker, ancestor file/directory markers, and a dangling marker symlink. During review, the positive case gained nested restoration assertions and target-side export gained its own direct regression. Malformed-success and non-NotFound metadata-error branches were source-reviewed, not separately fault-injected. Four integration drafts and the marker drafts preceded implementation; the fifth integration followed review feedback. No executed old-source failing run or mutation probe is claimed.

## Actual gate and evidence consistency

Inspected the exact commit with `git show` and compared the three scoped files against it: no difference from reviewed working files. [Run 39081716244](https://github.com/nessalabs/workspace-sdk/actions/runs/39081716244) completed successfully on Ubuntu 24.04 and macOS 15. Both original checkout logs identify the exact source SHA, and both jobs successfully complete `scripts/quality-gate.sh`.

The gate includes formatting, Clippy with warnings denied, feature-selected builds/tests, rustdoc with warnings denied, supervisor build, and explicit managed/PTY performance qualification. Individually counted passing names in the original log show all five new integration cases and both private tests twice per OS, under all features and no default features: 28 new executions total. Counts agree with the evidence summary. Reviewer commands only inspected source/evidence and validated this document; no local build or test execution was performed.

Checked `docs/evidence/git-capture-discovery-2026-10-17/qualification.json` against original `../workspace-cloud-run-39081716244.json` and `.log`. Job arrays match exactly; conclusions are success; source matches both checkout logs; the raw log SHA-256 matches recorded `e3000cdddd9d438c8199e5ea4f5f91c72589e2d4611bf0cf57cd14573f5faf62`. Independently parsed the two complete performance JSON objects per OS from the original log, removing only runner/step/timestamp prefixes. Both full arrays match the evidence summary exactly.

Each OS has three managed and fifteen PTY samples with debug assertions enabled. Linux used x86_64; macOS used aarch64.

| Environment | Managed cancellation, ms | PTY cancellation, ms | Retained managed / PTY bytes |
| --- | --- | --- | --- |
| Ubuntu 24.04 | 52.69–52.97 | 52.37–65.04 | 32,768 / 131,072 |
| macOS 15 | 66.75–129.09 | 55.50–190.30 | 32,768 / 131,072 |

Retention values are consistent across their respective samples. These qualify existing runtime behavior, not Git capture performance or a statistical regression baseline.

## Remaining limitations

Source files and Git metadata must remain quiescent. A probe followed by marker inspection cannot prevent concurrent marker disappearance or repository replacement. Ancestor markers can conservatively refuse scanning across discovery or mount boundaries; this is not a universal Git-failure classifier. Bare/no-marker fallback is legacy behavior, not a claim to preserve Git history, branches, indexes, or complete Git backup state. New export evidence covers a local filesystem helper, not SSH/Boat transport, offsite storage, or physical power loss. Existing manual handoff and Stop/Lost policies remain unchanged. This quality decision does not replace the separate adversarial review required by `AGENTS.md`.
