# Checkpoint scanner: adversarial acceptance

Decision: **FINAL PASS for the scanner increment** at exact source `2fdaf4b3f716aaa7a60196116af9fdfd8441a3be`, based on successful full revalidation with the initial failure retained below. No unresolved P1/P2 finding affects the scanner change. This does not certify resolution of the separately identified native-runtime race hypothesis.

Reviewer: adversarial subagent, separate from production implementation and code-quality review. Disclosure: this reviewer authored the five public integration tests before implementation, then independently reviewed production source. The implementation owner drafted two private overflow tests before the counter implementation. No executed old-source failing probe or mutation experiment is claimed.

## Source findings and disposition

The prior scanner collected and sorted all directory entries before enforcing its visit allowance, and repeatedly summed all selected file sizes after each insertion. The accepted change streams `ReadDir`, shares checked visit/selected-byte counters across recursion, and sorts the bounded successful exclusion report by path components. Selected entries remain in the existing `BTreeMap`.

Reviewed the actual scanner diff, public tests, private overflow tests and retained selection semantics. Iterator errors propagate; failed scans acknowledge no checkpoint/archive. Entry allowance is checked before processing each yielded result. Checked byte addition occurs before insertion and refuses overflow or exceeding the total allowance. File-count limits and final entry validation remain. Ignore-rule ordering, tracked-file handling, mandatory exclusion and symlink handling are unchanged. Component sorting recreates prior successful sorted-sibling DFS report order, including descendants of `a` before `a-b` and `a.b`; ordinary full-path sorting would be incorrect here.

**Explicit resource tradeoff, accepted:** the streamed recursive walker retains one open directory iterator per active recursion level. It can reach an OS descriptor limit sooner than the previous collect-and-close approach. Errors propagate and iterator ownership unwinds with RAII; there is no silent partial success or descriptor leak on ordinary return. No new depth guard was introduced and no unchanged deep-tree admission envelope is claimed. This was raised in source review and retained in qualification limits, rather than concealed by a constant-memory claim.

No additional actionable P1/P2 issue remained in the frozen scanner source. First-error ordering among invalid siblings may change with enumeration order; successful selected output and excluded report order are the preserved behavior.

## Actual evidence and initial failed gate

Inspected [successful full revalidation 39083692263](https://github.com/nessalabs/workspace-sdk/actions/runs/39083692263), actual checkout lines, downloaded run metadata and raw logs. Both Ubuntu 24.04 and macOS 15 checked out the exact source above and successfully completed `scripts/quality-gate.sh`: formatting, warning-denying all-target/all-feature Clippy, both feature selections, examples, warning-denying documentation and managed/PTY qualification.

Independently counted each of the five public tests and two overflow tests passing twice per OS: **28 new-test executions**. Cases establish creation-order-independent selected entries/fingerprint/archive and exact DFS exclusion ordering across punctuation/Unicode prefixes; positive one-file policy with 1000 excluded entries accepted and 1001 refused; sibling-directory aggregate seven-byte acceptance/six-byte refusal; two-file acceptance/three-file refusal; and regular-file plus symlink-target byte accounting. Existing selection/Git/provider/recovery suites ran in the full gate.

The [initial run 39083477158](https://github.com/nessalabs/workspace-sdk/actions/runs/39083477158) **failed on Ubuntu**, while macOS passed. Preserve this failure: Ubuntu's unchanged `runtime_backend_host::local_adapter_checks_installed_runtime_launches_and_retains_no_input_secrets` failed at line 172 while executing a deliberately missing executable, after its training launch had succeeded. The error reported a disappeared supervisor after durable launch claim. All seven new scanner tests had already passed under both Ubuntu feature selections. The unchanged failing runtime case passed under both selections on both OSs during exact-source revalidation.

Read-only runtime source review identified a plausible pre-existing status/lock observation race: inspection reads active status, terminal publication can occur, then the observer sees the released lock and reports Lost using its stale status. The log does not prove that interleaving. This increment neither reproduced it deterministically nor fixed it; the retry is revalidation, not proof the failure was harmless or root-caused. The scanner is absent from this missing-executable command path and its integration test process has exited, but indirect scheduling effects are not excluded.

Verified the [qualification evidence](../evidence/checkpoint-scanner-2026-10-17/qualification.json) against both actual run metadata files and raw logs. Successful raw-log SHA256: `fa96042a3b075f63b20bbd7206722ea48d7e1a1995054a1339dbf233da74ac0e`. Initial failed raw-log SHA256: `ab3a362caf812f0ac77e7162a3c289eb13b9b928a1bc668c9fcb707e572fb4d8`. Each successful OS log contains full three managed and fifteen PTY sample objects matching the evidence exactly. No evidence discrepancy was found. This reviewer performed no builds/tests during acceptance and changed only this review record.

## Limits

- Public boundary tests establish selected/output/refusal semantics, not peak allocation, RSS, throughput, or the exact timing of internal accumulator checks. Source inspection establishes removal of per-directory collection and repeated aggregate summation.
- This is not constant-memory or wholly linear scanning. Selected bytes, entry maps and excluded reports remain retained; excluded reports still sort, tracked-prefix searches and recursive ignore-rule cloning remain, and open directory handles are O(depth).
- Custom engine bounds qualify local capture and `export_tree_archive`; they do not reconfigure native target defaults. The retained native helper suite ran separately in the same gate.
- No deep-tree descriptor benchmark, scanner speedup, statistical regression baseline, live provider, offsite or power-loss qualification is claimed. The three managed/fifteen PTY hosted debug samples per OS concern existing runtime scenarios, on Linux x86_64 and macOS aarch64.
- Source files and Git metadata must remain quiescent. No atomic snapshot, new recovery policy, scheduler, registry, coordinator or deployment guarantee is introduced.
