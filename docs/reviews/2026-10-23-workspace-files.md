# Workspace files first slice, issue 4

## Reviewed change and limits

Source reviewed: `962272b3424e266f557fda159231b42c075ed968`, based on readiness merge
`fa942f883c2888f70bd46a32b53546497ea85133`. PR3 was not modified. Source commit
`28f946d3c49e874119bc8081a5e85adf4032b51f` introduced the slice; the final source
commit addresses two nonblocking review suggestions.

This passes the qualified first slice: provider-neutral optional APIs, Local Linux
bounded listing, revision observations, caller-driven directory reconciliation,
and no-clobber creation. Full collaborative editing is not complete. No new merge
engine was introduced. Existing GitCheckpointEngine owns conflict preservation,
review, resolution, guarded apply and recovery; that apply still requires actual
writer quiescence. The [state contract](../state/files.md) records unsupported
replace/delete/rename, metadata-token semantics, polling and provider limitations.

## Independent review loop

Separate agents reviewed the actual diff, standards and tests, without editing:

- Quality reviewer `/root/files_quality_review`: Clean Code, SOLID, dependency
  direction, cohesion and pattern judgment. Initial FAIL pending Q1–Q3. Re-review
  PASS at `28f946d`; final delta PASS at exact source `962272b`.
- Adversarial reviewer `/root/files_adversarial_review`: races, unsafe replay,
  boundaries, cleanup and performance. Initial FAIL pending A1–A4; independently
  executed E2E, malformed-provider, crash-cleanup and performance tests. Re-review
  PASS at `28f946d`; final delta PASS at exact source `962272b`.

| Finding | Severity | Fix and closure |
| --- | --- | --- |
| Q1/A2: empty advancing pages could loop forever | P2 | Shared page validation, exact cursors and at most 16 pages. Malformed-provider regressions pass; both reviewers closed. |
| Q2/A4: bounds checked after transport | P2 | Shared pure validation at controller and native boundary. No-dispatch regression passes; both reviewers closed. |
| Q3/A1: non-timeout lost acknowledgements appeared definite | P2 | Explicit native PublishRejected; all invocation/parse failures preserve Unknown. Lost/malformed/failed response regression passes; both reviewers closed. |
| A3: named staging could survive process death | P2 | Anonymous O_TMPFILE and exclusive linkat; no named staged data. SIGKILL cleanup regression and independent syscall probe pass; adversarial reviewer closed. |
| Token semantics and special-file coverage | P3 | Metadata limits documented; FIFO fixture rejects read. Closed. |
| New-path performance coverage | P3 | Three samples of complete 4096-entry polling, retained logs and cadence/backoff guidance. Closed. |
| Duplicate observer checks and malformed Created token | P3 | One shared per-page validator; malformed created revision becomes Unknown with regression. Final source delta passed both reviewers. |

No unresolved P1/P2 or other actionable findings remain. Review decisions apply
to the qualified scope only. Initial reviewer decisions were explicitly conditional
on completion of the full gate and docs validation; those prerequisites subsequently
passed as recorded below.

## Executed checks

Environment: Linux 6.18.55 x86_64, overlay filesystem, Rust stable 1.99.1, debug
builds, synthetic TempDir workspaces in this cloud execution environment. No live
user workspace, destructive resync, migration, remote-provider mutation or secrets
upload was used.

- `scripts/quality-gate.sh` passed on final source `962272b`: rustfmt, all-feature
  all-target Clippy with warnings denied, example/supervisor builds, all-feature
  and no-default-feature tests, rustdoc with warnings denied, runtime and PTY
  performance qualification. [Complete log](../evidence/2026-10-23-workspace-files/quality-gate.log).
- `cargo test --locked --test workspace_files --test provider_transports` passed:
  10 file E2E tests plus 9 transport tests, including 3 new malformed-provider
  contract regressions. [Log](../evidence/2026-10-23-workspace-files/files-e2e-and-transport.log).
- `cargo test --locked --lib file_browser` passed: 2 domain regressions, 1 actual
  process-death cleanup regression, and its no-op parent-side child-test helper.
  [Focused log](../evidence/2026-10-23-workspace-files/files-unit.log); final-source
  gate reran the same tests.
- `cargo test --locked --test workspace_files -- --ignored --nocapture` passed.
  4096-entry first polls: 1202/1151/1162 ms; unchanged polls: 1156/1167/1154 ms.
  The independent reviewer measured about 1.1–1.2 seconds too. These few samples
  are not a statistical regression baseline. [Log](../evidence/2026-10-23-workspace-files/files-performance.log).
- Nessa Docs catalog validation passed: 7 pages, no issues.
  [JSON](../evidence/2026-10-23-workspace-files/docs-catalog.json).
- Rendered browser verification passed 9/9 checks with system Chromium: root
  diagram drill-down, both rendered charts, exact sourceRevision, authored Unknown
  explanation, existing checkpoint workflow drill-down, mobile overflow and no
  page errors. [JSON](../evidence/2026-10-23-workspace-files/docs-browser.json),
  [desktop](../evidence/2026-10-23-workspace-files/files-desktop.png),
  [mobile](../evidence/2026-10-23-workspace-files/files-mobile.png),
  [rerunnable harness](../evidence/2026-10-23-workspace-files/verify-docs.mjs).
  Screenshot review uses Fit and closes the mobile sidebar. Renderer appearance
  outside those states is not claimed. The renderer was built from a separate
  cloud checkout. The new owning state page records final source `962272b`; the
  root received navigation only, and unrelated pages were not advanced.
- `git diff --check` passed. Source/test module exports and docs links inspected.

## Remaining product scope and merge guard

The API does not expose native update/delete/rename mutations or a managed edit
session because current targets cannot fence arbitrary editors. A future session
must reuse checkpoint reconciliation and prove writer authority, or use provider
conditional publication. No CRDT library is selected. Current observation is
current-state polling, not lossless push notification. Dirs above 4096 children,
nonportable names and non-Linux native browser operations are explicitly unsupported.
Only Local Linux is newly E2E-qualified; remote live behavior remains unverified.
MacOS CI must pass the honest unsupported-target path; no local macOS execution
was available here.

Deliver as a draft PR. Parent owns final exact-head guarded merge, including
checking current base, required Linux/macOS CI and review disposition. Do not
interpret local acceptance as permission to merge a later unreviewed source head.
