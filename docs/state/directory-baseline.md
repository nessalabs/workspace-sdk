---
id: "directory-baseline"
title: "Publish only complete directory observations"
kind: "statechart"
status: "implemented"
summary: "A failed or cancelled directory poll retains the previous complete baseline."
parent: "workspace-sdk"
sourceRevision: "867a4c15e97e025a1066b3c0cd133ffa81f374f1"
sources:
  [
    "src/application/directory_observer.rs",
    "src/domain/workspace/file_browser.rs",
  ]
diagramLinks: {}
diagramDetails:
  [
    {
      "diagram": 0,
      "node": "Reading",
      "section": "Example: the second page changed",
    },
  ]
model:
  {
    "spec": "tla/directory-baseline.tla",
    "config": "tla/directory-baseline.cfg",
  }
modelCheck:
  sourceRevision: "867a4c15e97e025a1066b3c0cd133ffa81f374f1"
  checker: tlc-1.7.4
  date: 2026-11-03T08:45:30.175Z
  config: tla/directory-baseline.cfg
  modelDigest: cab51a66d8d6f312a543feae2d6d8a611e338189e002a5b80c97fe6a4d03d6fd
  states: 27
  held:
    - ConsistentPublication
    - AbortPreservesBaseline
    - CompletePublication
  broke: []
  unchecked: []
---

When a directory listing changes halfway through a poll, keep the previous complete view. The observer publishes a new view only after every page is valid.

## Example: the second page changed

The first page was read at revision A, then another writer changed the directory before page two. The poll returns Conflict and keeps the old complete baseline. Keep showing the old view as stale and retry a whole poll; do not merge the partial first page into it. The next successful result contains current differences, not every event that occurred between polls. Dropping the observer and constructing another deliberately yields a reset on its first successful poll.

## Progress and ownership

```mermaid
stateDiagram-v2
    [*] --> Idle
    Idle --> Reading: begin poll
    Reading --> Reading: next valid page
    Reading --> Idle: error or cancel / discard partial snapshot
    Reading --> Published: final valid page / replace baseline
```

An observer has no background task: the caller drives polling. It gathers at most 16 pages and 4,096 immediate children, rejects duplicate paths and nonadvancing cursors, and requires one revision across the entire poll. Only after the final page does it compute upserts/removals and replace its baseline. The first success is a reset. A new observer after restart starts fresh; it cannot reconstruct transient events between polls. Rename is removal plus upsert.

## Model coverage

The model has an old baseline and a two-page candidate with independently chosen revisions. Receiving pages advances an independent count; publication records the count actually received, rather than assigning a completed count. It checks that published candidates contain both pages with one revision, and aborted polls preserve the baseline. This abstracts real validation, byte/path bounds and full directory contents. [File browsing](files.md) details revision-bound reads and Linux-only exclusive creation.
