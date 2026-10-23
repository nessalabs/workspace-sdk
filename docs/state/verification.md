---
id: sdk-verification
title: Verification and remaining limits
kind: guide
status: reference
summary: Source checkpoints and test boundaries distinguish implemented behavior from qualified deployments.
parent: workspace-sdk
sources:
  - scripts/quality-gate.sh
  - docs/plan.md
  - docs/performance.md
  - tests/boat_live.rs
  - docs/evidence/vercel-linux-pty-artifacts/index.json
diagramLinks: {}
---

The source map starts from `0b7bbc0a9be0cce0713df05be56f2a63d6c14c30`, the merged workspace-sdk main observed on 2026-10-23. PTY integration and live qualification PRs #1 and #2 were already merged before this readiness task. The original checkout's historical review-file edit is excluded from this task's isolated branch.

The [readiness review](../reviews/2026-10-23-sdk-readiness.md) records current commands, source revisions, fixes, independent decisions, and remaining failures. Earlier [live Linux artifacts](../evidence/vercel-linux-pty-artifacts/README.md) retain their historical source and environment identities; they are not evidence for later code bytes.

## Required gate

Run `scripts/quality-gate.sh` for formatting, strict Clippy, probe builds, both feature configurations, warning-free Rustdoc, supervisor build, and both performance suites. The repository also requires separate code-quality and adversarial reviewers. A mechanical pass alone does not pass acceptance.

## Evidence boundaries

Local fixtures exercise real processes, retention, reconnect, cancellation, and cleanup. Provider transport contracts use simulated boundaries. The historical Vercel run exercises LocalProvider inside a capability-free Linux VM, not Boat, SSH, container, or Vercel provider deployment. Real native agent authentication and inference, live SSH/container qualification, and registry distribution remain unverified or deferred. Boat accepted deletion may remain blocked; report the actual returned state.

Performance suites record a few local samples and bounds. They do not establish a statistical regression baseline, peak memory, or long-duration leak freedom. See [performance coverage](../performance.md). No claim in this map upgrades an unsupported or unrun scenario to a passing test.

## Documentation checks

Catalog validation must resolve stable IDs, parents, source paths, headings, and drill-down targets. Render every diagram and inspect navigation. Record the tool result and any visual exclusions in the readiness review. The source revision is recorded in prose because Nessa Docs rejects unknown frontmatter fields.
