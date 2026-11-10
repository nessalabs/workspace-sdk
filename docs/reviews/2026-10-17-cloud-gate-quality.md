# Manual cloud gate: independent quality review

**PASS for the one-line workflow change. No P1/P2 or actionable lesser findings.**

Reviewer: independent `workspace_handoff_quality_review` agent, round 1.
Reviewed base `afbea4d7d6df77f9acf4ef0bb69839488b990085` and the actual
uncommitted diff: add `workflow_dispatch:` under `on` in
`.github/workflows/ci.yml`. Read the complete workflow, `AGENTS.md`, and
`coding_standards.md`. This reviewer made no source changes, ran no builds,
and dispatched no cloud work. The separate adversarial review is required.

The existing workflow already owns the shared `scripts/quality-gate.sh`
invocation and Linux/macOS matrix. Adding its standard manual trigger reuses
that implementation without copying build commands, adding a second CI
abstraction, modifying runtime code, or coupling the SDK to GitHub APIs.
Existing main-push and pull-request triggers remain intact. Checkout, cache,
toolchain setup, job matrix, and `contents: read` permissions are unchanged.
No new credential input, secret handling, repository write permission, or
deployment behavior is introduced.

Evidence: independently inspected the exact diff and complete workflow;
`git diff --check` returned success. The parent reports actionlint success and
the preceding full Rust/local-performance gate on unchanged native inputs.
Those checks establish syntax/readiness and prior source qualification, not
execution of this newly dispatched branch in the cloud. No redundant Rust
build was run by this reviewer for the trigger-only change.

Acceptance here means the manual trigger is a minimal, maintainable change
ready for a dispatch attempt. Availability of repository credentials, GitHub
workflow registration/dispatch rules, hosted runner capacity, and actual job
results still require the parent's real remote attempt. A refused dispatch
must be reported as a blocker rather than a completed cloud run. A successful
dispatch alone is not a passing remote qualification; record the run's commit,
matrix results, and final conclusion before making that claim.

This change does not implement automatic workspace failover, provider
switching, priorities, deployment, or a feature-branch merge into main.
No fixes or re-review were required for the reviewed scope.
