# Retained fingerprint inspection: independent quality review

Decision: **PRECLOUD QUALITY PASS**. No unresolved P1/P2 finding. Full exact-candidate cloud qualification and separate adversarial acceptance remain required for final acceptance.

I reviewed the actual engine port/adapter/parser reuse, shared archive acquisition, retained-admission call site, new tests and documentation. After the exclusive probes ended, all ten current frozen hashes independently matched `work/fingerprint-final-source.json`, SHA256 `c7a02a041298690e0cca5ae61e61cfd86e1ad6f887778762b9b05e605e157a5b`. I ran no builds/tests/probes and edited only this review.

The built-in Git engine derives the fingerprint only after the existing complete bounded archive parser validates contents, digests, sizes, paths, selection policy, modes and symlinks. This path uses immutable limits and no Git calls, checkpoint publication or mutable engine state. The compatibility default deliberately calls capture_archive exactly once with default options, returning its fingerprint and propagating errors; it is explicitly documented as potentially persisting an artifact. There is no hidden fallback after override errors. Existing engines remain compatible rather than acquiring an unexpected Unsupported failure.

Shared acquisition retains the complete transfer byte bound, declared-length/cursor/checksum validation and success/error cleanup attempt. Cleanup before engine consumption is safe because the controller already owns the verified bytes; its existing best-effort limitation is stated. Retained admission still checks the successful predecessor, canonical checkpoint, recovery lease, expected epoch/head, all-writer exclusion and retry identity. Prelaunch retries revalidate selected state. Publication still creates its durable checkpoint.

Actual local passing evidence is split across completed binaries, not a single179-green run: the second run passed145 library, three engine, seven remote-tree and eleven retained-continuation cases before later native test failures; the fourth run passed the corrected three new native cases and ten workflow cases. These total179 affected passing cases across successful binaries. The first run failed compilation because a new test accessed a private fixture field. The second run exposed two test-oracle errors; the third repeated the full-session equality failure because an intended text replacement had not applied. These failures remain recorded and are not counted as passing runs.

The reviewed correction replaces invalid full-session equality with exact pinned request/provider/epoch/predecessor/workspace/run/checkpoint and complete retained-provenance equality, unchanged canonical head, phase only Preparing or Restored, preserved edited bytes, and no successor/artifact. It permits legitimate durable prelaunch phase reconciliation without allowing Started. The cursor-fault assertion now matches the existing earlier backend guard and logs its actual outcome. No production weakening was needed for these test fixes.

Three host-linked probes reached actual runtime assertion failures with exit101: restoring discarded capture in admission adds an unexpected checkpoint; bypassing transfer checksum binding accepts tampered selected content; trusting archive metadata fails malformed-archive refusal. Cargo rebuilt the mutated controller-linked libraries; these mutations are not supervisor-operation changes. All source was restored. A restored supervisor build then preceded all six new tests, which passed (three engine3.22s, three native6.70s). All-feature/all-target strict Clippy10.10s, strict all-feature documentation4.12s and formatting passed. Successful bounded reports exited0 with no stop/cleanup error and owned process groups absent; native fixtures supply the distinct detached-execution ownership evidence.

Observed same-archive 2MiB engine comparisons each show one capture artifact,13 capture Git calls and about4.20MB capture state growth versus zero added artifact/state bytes/Git calls for fingerprinting. Capture observations were578.306ms/563.370083ms and fingerprint64.346334ms/64.150833ms. The actual native same-tree comparison recorded681.577667ms capture versus549.208542ms inspection, with two retained admissions768.46875ms/740.57725ms. These are functional debug observations, not a statistical speedup baseline. Full selected-tree download, bounded buffering and canonical/target Git work remain; no bandwidth, peak-memory, streaming, deduplication or COW improvement is claimed.

Raw evidence SHA256:

| Evidence | SHA256 |
| --- | --- |
| Second run | 66339b2e4c6099f1a568253fe088fc77d42a95202e07e935d4b47fa600734d25 |
| Fourth run | 312d9df427c6e22551008d442e0ca7e1f2e0ba0f55a878c5dfb96ef9eb33c36c |
| Restored supervisor build | 07d8927a806da5e3b35f18beb057174faf50a56422308d9aa033f7cd84882b0e |
| Restored six tests | e13fe467ed30bf210ab182a92a5f7938445c63eae224352c253d730df06114e9 |
| Strict Clippy | 0552c431d99fb33dc38de47e29cd3f6247111fa69629f937aac714c0b5537bd3 |
| Strict docs | 84b82d3983ed0683e534a70ca5c29771b153688ad566e5b8c8d2d49057fc6d1e |

The [credited LayerFS study](../research/layerfs-learning.md) identifies the inspiration and distinguishes preexisting local safety behavior from this applied optimization. No upstream code or dependency was imported. This increment neither weakens acknowledged recovery requirements nor qualifies live remote transport, physical failure, automatic failover or application migration. No-artifact inspection is a built-in Git-engine guarantee, not a universal promise about custom engines. Full cloud evidence is still pending.
