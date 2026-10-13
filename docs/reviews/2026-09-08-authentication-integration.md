# Authentication integration preparation — 2026-09-08

Scope: provider-neutral native login control, host-scoped process identity,
preparation store coordination, managed-backend output/cancellation, and the
mandatory quality gate. The separately owned PTY dependency remains incomplete;
this review does not accept real PTY behavior or native credential flows.

## Independent reviews

- `code_quality_gate`: strict Clean Code, SOLID, dependency direction, cohesion,
  domain invariants and design-pattern review. Final decision: **PASS**, no
  unresolved scoped P1/P2 findings. Independently reran the real backend Unicode
  regression (one test passed).
- `adversarial_auth_gate`: independent race, identity, ambiguous delivery,
  cancellation, boundary and misleading-evidence review. Final decision:
  **PASS**, no unresolved scoped P1/P2 findings.
- `performance_gate`: implemented the local managed-process qualification;
  its assertions received independent design and adversarial review, both PASS.

## Findings and fixes

| Severity | Finding | Resolution and re-review |
| --- | --- | --- |
| P1 | Process references could alias another target with the same run ID. | References retain validated backend scope; receipts and adapter operations reject foreign scope before I/O. Real two-workspace control test passes; both reviewers pass. |
| P1 | Service-instance locking did not coordinate controllers sharing a store and could serialize unrelated preparations. | Required per-preparation store leases, memory ownership and filesystem locks; save/load identity validation and uncertain launch reconciliation. Shared-store and failed-save tests pass; both reviewers pass. |
| P2 | Output validation confused decoded UTF-8 length with raw cursor bytes. | Limit is 2..=128 KiB raw; decoded expansion bounded to 3x, characters bounded to raw limit, non-gap cursor advance bounded, regressions rejected. Real service/backend split-Unicode test passes; both reviewers pass. |
| P2 | Performance page checks could accept twice the combined byte limit. | Assert combined stream bound; both reviewers pass. |
| P2 | Empty retained-output drain could satisfy performance assertions. | Assert exactly 16 KiB retained per stream, expected bytes, and final offsets at least pre-cancel offsets; both reviewers pass. |

Controls retain the exact current attempt through lost state or uncertain cancel.
Input can return accepted-prefix or unknown delivery and is never replayed.
Status/control calls have finite budgets; receipts and Debug omit transient input.

## Validation

`scripts/quality-gate.sh` runs formatting, strict all-target/all-feature Clippy,
all-feature tests, no-default-feature tests, warning-free Rustdoc, supervisor
build, and the ignored local performance qualification. Final run: **PASS** (exit 0): 107 all-feature Rust tests and 3 doctests;
103 no-default-feature Rust tests and 3 doctests; strict Clippy, formatting,
Rustdoc, supervisor build and performance qualification all passed.

Performance evidence: [three local samples](2026-09-08-runtime-performance.json)
on macOS/aarch64, debug build, 32 KiB retention and at least 128 KiB emitted
per stream. Observed launch 28–40 ms, first output 67–269 ms, loaded reads
2.4–2.7 ms, and cancel 65–73 ms. All samples retained exactly 16 KiB of each
stream and passed bounded-page/content/offset checks.

## Remaining limits

- Backend input/resize and mandatory-terminal launch return Unsupported until
  the completed PTY dependency is integrated and verified.
- Authentication transcript decoding is lossy at UTF-8 page boundaries; it is
  unsuitable as terminal emulation state.
- Three debug-build local performance samples are descriptive observations,
  not a statistical regression baseline. CPU/RSS, PTY input/resize, cloud
  latency and live credential flows are not qualified here.
- This change does not claim new remote live-test evidence. Historical Box
  evidence predates these preparation changes.
