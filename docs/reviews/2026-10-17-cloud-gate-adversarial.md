# Manual cloud quality gate: adversarial review

Reviewer: independent agent `workspace_handoff_adversarial_review`.

**PASS for the small configuration-readiness change. Remote execution is still
pending and this review does not accept a completed cloud qualification.** No
unresolved P1/P2 affecting this one-line change was found.

## Actual reviewed change

Base `afbea4d7d6df77f9acf4ef0bb69839488b990085`, branch
`codex/workspace-portability`. The actual diff adds only `workflow_dispatch:`
to the existing `on` mapping in `.github/workflows/ci.yml`. I read the current
YAML, unchanged `scripts/quality-gate.sh`, repository review instructions and
cached `origin/main` workflow. No production Rust or separately owned PTY
implementation changes are included. This review writes only this record and
runs no builds, probes, dispatches or cloud operations.

The parent reports actionlint and diff checks passed. The unchanged Rust scope
already has the completed local mechanical gate, performance samples and both
independent reviews recorded for the preceding increment. Those facts support
local readiness; neither local results nor the earlier successful main matrix
run `38353980700` establish that this new feature-branch commit ran remotely.
I have not independently queried that cloud run during this review.

## Event, ref and authority assessment

The new manual event is a sibling of `push` and `pull_request`. The push filter
remains `main`; it does not filter manual runs. An explicit manual dispatch
against the intended feature ref is therefore required. The workflow has no
inputs interpolated into shell code, no new secrets, no write token, no
`pull_request_target`, no deployment job and no new environment privileges.
`permissions: contents: read` is unchanged. The checked-out ref's script is
executed on the existing Ubuntu 24.04/macOS 15 matrix, so a dispatcher must
select the intended branch and verify the resulting `head_sha`, event and both
job conclusions. A moving branch name alone is not immutable test evidence.

Manual dispatch permits authorized dispatchers to choose a ref and run its
code. That is the intended capability, not a claim that this is a safe runner
for arbitrary untrusted code. Existing checkout credentials, mutable action
version tags and cache behavior are unchanged; this line supplies no new
protection or bypass. Duplicate manual runs may consume additional hosted
minutes; there is no new automatic schedule or self-triggering loop.

## Default-branch availability is an execution prerequisite

The cached `origin/main` workflow read during this review contains the existing
file but lacks `workflow_dispatch`. GitHub documents default-branch availability
for manual events, while its CLI supports an explicit non-default `--ref`.
See [GitHub's trigger documentation](https://docs.github.com/en/enterprise-cloud%40latest/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow)
and [the manual-run guide](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow).
Consequently, committing and pushing this line only to the feature branch must
not be reported as sufficient proof that GitHub accepted a dispatch. The parent
must preserve the actual server response and identify the resulting run. A
server refusal or missing event availability blocks the requested remote
qualification, even though the YAML itself is valid. Any remedy involving the
default branch requires the separately applicable authorization; this review
neither authorizes merging nor treats an unavailable dispatch as success.

## Acceptance boundaries

No code defect or unsafe authority expansion was found in the one-line change,
so no fix/re-review round is required for configuration readiness. Final cloud
acceptance still needs the pushed commit, an actual accepted dispatch to that
ref, an exact run SHA match and successful Ubuntu/macOS quality jobs. Queued,
cancelled, failed, skipped or mismatched runs do not satisfy that gate. Hosted
performance samples describe their actual runner environment and remain small
qualifications, not a stable benchmark baseline. This review does not claim
remote provider behavior, deployment, secret isolation, or cloud success.
