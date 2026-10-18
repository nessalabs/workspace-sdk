# Retained fingerprint inspection: adversarial review

**PRECLOUD ADVERSARIAL PASS. No unresolved P1/P2 finding; exact-source full cloud qualification remains pending.** Independent reviewer: `workspace_handoff_adversarial_review`. Implementation and tests were authored by other agents. This reviewer changed only this record and ran no checks/probes.

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
