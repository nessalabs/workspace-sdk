# Managed editing and portable observation, issue 4

## Scope and source

Exact source reviewed: `e0ed9670326bb4fd24a2043dd0a1110b8c26f793`, based on
merged PR5 `db98741b2166e2f9f829eeac1bd64da65e033ef1`. PR5 is unchanged.
Implementation source `68ef014` is followed by a test-only portable fixture
refinement `e0ed967`. This follow-up adds:

- A local ManagedEditSession with immutable candidate revisions, staged binary
  create/replace/delete/rename and the existing checkpoint review/resolution/apply.
- Explicit caller quiescence gates, shared nonblocking root-derived workflow
  leases, directory identity binding and engine-enforced resource bounds.
- A checkpoint-owner apply preflight protecting uncaptured human data, including
  ignored destination files, before recovery intent or mutation.
- Descriptor-relative POSIX observation for Linux/macOS; native live publication
  remains Linux-only. No new merge/CRDT engine, remote edit transport or UI.

The [managed-edit contract](../state/managed-editing.md) distinguishes enforced
coordination from unmanaged-writer assumptions, supported restart inspection and
remaining issue 4 scope. It is a batch review/save workflow, not unattended CAS
against arbitrary editors or per-keystroke collaboration.

## Independent reviews and closure

Separate reviewers inspected actual source, standards and executed evidence,
without editing implementation files:

- Quality `/root/files_quality_review`: initial FAIL for ignored-file loss and
  exhausted completion capacity; independently compiled/reproduced ignored-file
  loss before the fix and reran the probe after it. Final exact-source PASS at
  `e0ed967`, with no unresolved quality findings.
- Adversarial `/root/files_adversarial_review`: initial FAIL for budgets/bounds;
  independently executed bounds probes and tests, examined resource/portable
  failure paths. Final exact-source PASS at `e0ed967`, no unresolved P1/P2.

| Finding | Severity | Fix and re-review |
| --- | --- | --- |
| Q1: ignored human file removed by automatic rollback after candidate collision | P1 | Checkpoint-owner preflight rejects unselected result/ancestor collisions before journal or mutation. Real regression asserts original bytes and NotStarted. Independent original probe now preserves bytes. Closed. |
| Q2/B1: last accepted stage consumed its review/resolution budget | P2 | Separate limits of 32 staging, 16 review and 16 resolution attempts; reject new candidate/review when completion capacity is exhausted.Last-stage regression completes conflict resolution and apply. Closed. |
| B2: candidate-only bounds allowed oversized live captures/resolutions | P2 | ICheckpointEngine exposes effective enforced limits; session admits only configured strict bounds before capture. Oversize live/review tests reject without retained oversized checkpoints. Closed. |
| B3: ancestor catalogue could allocate quadratically and component metadata was unbudgeted | P2 | Prefix query uses existing ordered catalogue; shared visit budget covers metadata components and child enumeration before mutation. Portable16file/64component fixture reaches cap and preserves data. Closed. |
| B4: huge mutation paths copied before validation | P2 | Pure EditChange validation checks all path/from/to fields before clone/serialization; input regressions preserve candidate and snapshot count. Closed. |
| Resolution acknowledgement size | P3 | Count 4096/length 4096 admission before engine set construction. Closed. |
| Linux-only deep fixture would exceed macOS PATH_MAX | P2 portability | Replaced the long path with 16 files under 64 short components; same visit bound within portable path lengths. Exact-source re-review passed. |

All source findings are closed. Both reviewers explicitly passed final source.
Their final decisions required full-gate/docs evidence completion and actual
macOS CI for platform qualification; those are reported separately below.

## Executed local evidence

Linux 6.18.55 x86_64, overlay filesystem, Rust stable 1.99.1, debug builds. Only
synthetic TempDir workspaces were used. No live user data, migration, destructive
resync, remote provider mutation or secrets upload occurred.

- Targeted real-engine integration: 15 managed-session tests, 13 checkpoint-engine
  tests and 3 portable subprocess tests passed.
  [Log](../evidence/2026-10-23-managed-editing/targeted-tests.log).
- 3 native preflight unit tests passed, including regular/symlink/empty-directory
  collision refusal, excluded-child directory transitions and portable traversal
  refusal before mutation. [Log](../evidence/2026-10-23-managed-editing/preflight-unit.log).
- Synthetic `cargo run --locked --example managed_edit` passed, demonstrating
  stage, preserved human/agent conflict, manual acceptance and explicit apply.
  [Log](../evidence/2026-10-23-managed-editing/example.log).
- 3 performance samples over 257 files/at least 1 MiB: begin 1175/1173/1163 ms,
  stage 462/455/445 ms, review 3783/3900/3759 ms, apply 1526/1561/1525 ms.
  [Log](../evidence/2026-10-23-managed-editing/edit-performance.log).
- 4096-entry observation samples: initial 1073/1076/1080 ms,
  unchanged 1088/1108/1081 ms. [Log](../evidence/2026-10-23-managed-editing/browser-performance.log).
  These few samples are not statistical regression baselines or macOS/remote
  latency qualification. Hosts must use a blocking executor and batch edits.
- Full `scripts/quality-gate.sh` passed on exact source `e0ed967`.
  [Gate log](../evidence/2026-10-23-managed-editing/quality-gate.log) covers
  rustfmt, all-feature all-target Clippy, all/no-default tests, rustdoc, example
  and supervisor builds plus existing runtime/PTY performance qualification.
- Nessa Docs catalog validation passed: 8 pages, no issues. Rendered browser
  verification passed 12 checks: root navigation, chart rendering, exact source
  checkpoint, candidate/recovery explanations, existing conflict-owner drill-down,
  mobile overflow and portable documentation.
  [Catalog](../evidence/2026-10-23-managed-editing/docs-catalog.json),
  [browser JSON](../evidence/2026-10-23-managed-editing/docs-browser.json),
  [desktop](../evidence/2026-10-23-managed-editing/managed-edit-desktop.png),
  [mobile](../evidence/2026-10-23-managed-editing/managed-edit-mobile.png).
  The repository-linked nessa-docs skill was used to trace source and author
  only affected owning pages; their sourceRevision names `e0ed967`.

## macOS qualification and final merge

The same portable subprocess tests exercise actual listing, revision reads,
external observation, symlink/FIFO rejection and stream cleanup on macOS CI.
Managed session integration and preflight tests also execute there. Linux
execution does not substitute for those results.

A local cross-check was attempted, but the existing pty-runtime build compiles
its target guardian executable, requiring an Apple linker/SDK. This Linux host
has neither. [Diagnostic log](../evidence/2026-10-23-managed-editing/macos-cross-check-unavailable.log).
No dependency source was patched or guardian artifact substituted. Actual macOS
CI remains required; an Unsupported-only path is not qualification of this feature.

Draft publication follows local source review/gates and docs checks. Parent owns
final exact-head guarded merge after current-base and terminal Linux/macOS CI.
Keep issue 4 open for actual enforcement against arbitrary live editors, remote
managed editing, durable session resume and push/event-log capabilities.
