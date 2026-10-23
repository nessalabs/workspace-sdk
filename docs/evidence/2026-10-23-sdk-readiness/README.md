# SDK readiness evidence

[index.json](index.json) records exact source revisions, commands, environment,
and original/published byte hashes. Only local checkout/toolchain path prefixes
are normalized. Performance JSON is unchanged. The required gate and runnable
local workflow completed with exit zero. Raw logs include the test names and
measurements; the [review](../../reviews/2026-10-23-sdk-readiness.md) owns the
acceptance verdict and limitations.

The initial gate overlapped regression authoring. It reproduced cancellation
failures after the tests were introduced; do not describe it as a clean baseline
pass. The Clippy red log preserves the two fixture-style failures corrected
without suppressing the strict gate.

Reproduce from the implementation revision with a supported POSIX host, Git,
Rust, and its pinned dependencies. Run the exact gate and local example commands
in the index. The supervisor argument is relative to the checkout. To validate
Nessa Docs, build that separate renderer and pass the SDK checkout as its project.
The original browser run used the renderer checkout available locally; its UI
inspection covers all six diagrams, drill-down, state details and Health. It is
not a live remote-provider or agent-authentication qualification.
