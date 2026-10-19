# Manual Linux live-test artifacts

The manual workflow now builds a musl Linux supervisor and the existing synthetic
Rust workload, so an explicitly authorized live test can use binaries from the
exact SDK revision. This change does not provision resources or run a live test.

Reviewed implementation: `8ca3b4a84a43d5a53f69ea03c3fbddd680fe8662`.
Changed implementation file: `.github/workflows/ci.yml`.

## Validation

- `git diff --check` and Ruby YAML parsing passed; the artifact job is restricted
  to `workflow_dispatch` and has a 30-minute timeout.
- [Cloud run 39268719179](https://github.com/nessalabs/workspace-sdk/actions/runs/39268719179)
  passed the complete existing quality gate on Ubuntu 24.04 and macOS 15,
  including their mechanical and performance checks, for the exact revision.
- The separate `live-artifacts` job passed its native musl builds and upload.
  Downloaded checksums matched both files. Independent inspection confirmed
  ELF64 little-endian x86-64 and absence of `PT_INTERP` in both binaries.
- Upload paths contain only those two public test binaries and their checksum
  manifest. The artifact name includes the source SHA; retention is three days.

| File | SHA-256 |
| --- | --- |
| workspace-supervisor | `4d5681b48c5bdc95125ed873e8cf904da2d75fa53ac84bce2acf47eb579ff1b4` |
| boat-live-workload | `47f9a4ba2ccdc0c9e111d0b52ad4429f3dd8641fd6da3837cffb3626c1fee9d1` |

## Independent reviews

The code-quality reviewer (`nessa_workload_quality`) and separate adversarial
reviewer (`workspace_handoff_adversarial_review`) independently passed the final
workflow and downloaded artifact scope after inspecting the exact source,
actual cloud results, checksums and ELF headers. No unresolved P1/P2 findings.
The adversarial P3 request for a bounded job duration was fixed with the
30-minute timeout before candidate dispatch and verified in final review.

## Limits

This gate qualifies artifact preparation. Native Boat execution and provider
cleanup require their own observed evidence. Binary format does not establish
remote kernel compatibility, deletion completion or billing cessation. The
workflow receives no Boat credentials and makes no Boat API calls. It changes no
SDK runtime or existing manual handoff behavior.
