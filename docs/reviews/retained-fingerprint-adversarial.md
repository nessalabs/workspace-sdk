# Retained fingerprint inspection: adversarial review

**FINAL ADVERSARIAL PASS for `741474ea91f797221ae26650f16ccad78d62e7b3`. No unresolved P1/P2 finding within retained fingerprint inspection scope.** Independent reviewer: `workspace_handoff_adversarial_review`. Implementation and tests were authored by other agents. This reviewer changed only this record and ran no checks/probes.

## Source and contract

The built-in Git engine fingerprints the same fully decoded selected-tree archive used for capture. Full parser, entry/path/mode/checksum/size/base64/symlink and configured limits remain in force. The override uses immutable limits and Tree fingerprinting without a Git process, checkpoint publication or engine lock. Canonical checkpoint export validation remains unchanged.

The additive port default captures exactly once with default CaptureOptions and returns its manifest fingerprint, preserving custom-engine compatibility, errors and possible persistence. Thus “no checkpoint/Git subprocess” is specifically the built-in Git override's guarantee; the general port is not promised side-effect-free.

SyncService shares archive acquisition between capture and inspection: declared length, cursor progression, total bound and SHA-256 checks remain. Best-effort transfer cleanup now completes before invoking the engine over owned bytes. This does not make cleanup infallible or add cancellation cleanup guarantees. Retained admission keeps predecessor success, exact canonical fingerprint, exclusive-writer requirement, epoch/head/provenance, existing lease and prelaunch retry validation. Publication still captures a durable checkpoint.

Tests qualify selected tracked-but-ignored content, exclusions, executable/symlink metadata and a 2 MiB binary; a disabled counting-Git wrapper proves inspection needs no Git process, while a failing capture control proves the wrapper is effective. Artifact count, engine bytes and Git invocation count remain unchanged by successful inspection. Malformed archive and custom compatibility/default-options/error cases are exercised. Real native continuation tests distinguish proof-only admission from durable publication, refuse divergence and prelaunch edited retries without successors, and reject modeled checksum/cursor corruption while removing staging.

## Initial failures and corrected oracles

The first attempt failed compilation on private fixture-field access; no tests ran. Tests were corrected to use a public Git engine at the same fixture artifact directory. The second command completed 145 library, three engine, seven remote and 11 retained-continuation cases, then failed two of three new native cases. One expected a later checksum/cursor error despite earlier backend cursor validation; the actual failing variant was initially unlogged. The other wrongly required whole-session equality even though Preparing-to-Restored progress is allowed before the selected-tree recheck. An intended phase edit was missed by a text replacement, so the third command repeated that failure (two native passes, one failure). All failed attempts remain retained.

The actual corrected assertion preserves canonical head and exact request/provider/epoch/predecessor/run/workspace/checkpoint/retained provenance, limits phase to Preparing or Restored, and requires Conflict, edited counter preservation and unchanged artifact/native claim counts. It relaxes only allowed prelaunch phase/revision progress. The fourth command passed all three new native cases and ten workflow cases. Together successful complete binaries cover 179 affected cases; there was no single all-green 179-case command.

## Semantic sensitivity and restored checks

Three parent-owned linked-library mutations each compiled and failed through the intended assertion (exit 101): restoring checkpoint capture during admission added an artifact; skipping transfer binding accepted corrupted selected-path bytes; trusting archive metadata accepted a malformed archive. These are actual semantic catches, not compile failures. `fingerprint-probes-proof.json` agrees with raw failure logs, and all ten final manifest hashes match after restoration. Manifest SHA-256: `c7a02a041298690e0cca5ae61e61cfd86e1ad6f887778762b9b05e605e157a5b`.

A restored supervisor build passed, followed by all six new tests with output enabled (three engine cases in 3.22 seconds and three native cases in 6.70 seconds; outer command 17.195 seconds). All-feature/all-target denied-warning Clippy, strict documentation and formatting passed. Each completed bounded-run metadata record shows exit zero, no stop/cleanup error and owned process group absent. Detached native ownership remains separately checked by the native fixtures.

Two actual measurement objects are retained: same-archive capture versus inspection, and native transfer/admission observations. They substantiate the tested artifact/Git-call difference; they are small local observations, not statistical throughput, memory or bandwidth baselines. Full archive export/download/buffering and validation remain, and canonical export still uses its existing machinery.

## Scope and decision

The credited [LayerFS research record](../research/layerfs-learning.md) distinguishes the learned inspection idea from preexisting SDK behavior. No upstream code/dependency was imported. This increment is not COW storage, delta transfer, off-machine durability, global writer fencing or live-provider qualification.

**Precloud PASS:** no unresolved P1/P2 source/evidence finding. Final acceptance awaits the exact-source full cloud gate. Prior failure logs, compatibility side effects, caller quiescence and cleanup/uncertainty limits remain part of this assessment.

## Final exact-source cloud decision

Independently inspected [run 39172464481](https://github.com/nessalabs/workspace-sdk/actions/runs/39172464481): both macOS 15 and Ubuntu 24.04 jobs and their full `scripts/quality-gate.sh` steps completed successfully at exact commit `741474ea91f797221ae26650f16ccad78d62e7b3`. All ten frozen source hashes match current files and that commit's Git blobs. Raw log SHA-256: `47a511c3d0efb9843e5df06ef17bc94e6f73d16b702e07d324041ccfbf65c7a9`.

Each of the six new test names has four actual successful executions in the raw log: both feature selections on each OS, totaling 24. Full gate formatting, denied-warning Clippy, documentation, builds, retained suites and performance qualification succeeded. Four complete performance JSON objects parsed from the raw log match the saved [performance evidence](../evidence/retained-fingerprint-2026-10-18/performance.json): three managed and 15 PTY samples per OS. These are retained debug-build qualification observations, not fingerprint throughput or statistical comparative benchmarks.

Reviewed the qualification draft against source and actual evidence: its split local coverage, failed attempts, three semantic catches, compatibility side effects and remaining transfer/cleanup limits agree. Its pending status is historical until final integration; this review independently closes the acceptance condition.

**FINAL PASS.** The exact-source full cloud result closes precloud qualification. All initial failed attempts and the split local pass history remain as recorded above; no retroactive all-green local command is claimed. Full transfer/buffering, canonical export work, custom-engine fallback side effects, exclusive-writer requirements and lack of COW/bandwidth/physical-failure claims remain explicit limits.
