# Checkpoint source scanner quality review

- Reviewer: independent quality/design agent `/root/boat_cleanup_quality`.
- Exact source: `2fdaf4b3f716aaa7a60196116af9fdfd8441a3be`.
- Scope: streaming source-directory scanning, checked shared scan budgets, final excluded-report ordering, and seven new scanner tests. Only `scanner.rs` and `tests/checkpoint_scanner.rs` change in the reviewed commit.
- Final decision: **PASS for this scanner increment**. No unresolved P1/P2 finding affects the reviewed scanner change. A separate unresolved runtime observation remains recorded below; this decision does not declare it fixed or qualify overall runtime stability without that limitation.

## Design and source assessment

The scanner consumes `ReadDir` directly, charging the visited-entry budget before filtering. One private concrete `ScanBudget` shares checked visited and selected-byte totals through recursive calls. Selected regular-file bytes and symlink-target bytes contribute to the same total. Overflow and exceeded limits retain the existing InvalidInput diagnostic categories; final `check_entries` validation remains. No new framework, public interface, identity, or source mutation is introduced.

Mandatory exclusions, workspace-ignore precedence, tracked-over-Git-ignore behavior, nested rule loading and directory pruning are unchanged. The selected `BTreeMap` retains stable entry/fingerprint ordering. Successful excluded reports are sorted by path components, preserving prior sorted-sibling depth-first order: `a/.env` precedes `a-b/.env`, unlike full-string sorting. First error among invalid siblings can change with filesystem enumeration order; no partial tree is returned on error.

The implementation removes per-directory sibling collection/sorting and repeated sums across all selected files. It does not make the complete scanner linear or constant-memory: selected content, excluded reports and final sorting still allocate, while tracked-prefix searches, rule cloning, recursion and other tree work remain.

Streaming keeps one open directory iterator per active recursive level. There is no new depth guard. Deep trees can therefore reach OS descriptor limits sooner than the previous collect/drop traversal; errors propagate and RAII unwinds the iterators. Deep-tree acceptance parity is unproved, and this tradeoff is an explicit limitation rather than a measured improvement or a descriptor-leak finding.

## Test assessment

Five public capture/archive tests were drafted before production changes. They establish exact component-wise excluded ordering across punctuation/Unicode prefixes and reverse creation order, identical selected entries/fingerprints/archives, the positive `max_files: 1` traversal allowance with 1,000 versus 1,001 mandatory-excluded entries, exact aggregate-byte and selected-file limits across sibling directories, and combined regular/symlink-target byte accounting. Failed captures retain prior artifacts and source payloads and do not publish another checkpoint.

The byte and file-count fixtures isolate their respective bounds. Public assertions establish accepted/refused behavior; final `check_entries` can also enforce those bounds, so these tests do not independently prove accumulator timing or allocation behavior. The two private overflow cases use supported positive maximum limits to verify actual checked arithmetic and InvalidInput refusal. Existing ignore/tracked-selection tests remain in the full gate. No executed old-source failing or mutation probe is claimed.

## Findings and disposition

| Severity / scope | Finding | Disposition |
| --- | --- | --- |
| P1/P2, scanner | No actionable blocking correctness, cohesion, dependency-direction or test-quality finding identified. | Source review passed before execution; exact-source full revalidation and all new tests pass. Directory-handle pressure and unmeasured performance limits remain explicit. |
| P2 investigation, separate runtime path | The first gate's Ubuntu no-default-feature `runtime_backend_host::local_adapter_checks_installed_runtime_launches_and_retains_no_input_secrets` failed at `tests/runtime_backend_host.rs:172` when executing a deliberately absent executable. It received `State("supervisor disappeared after durable launch claim; execution outcome is unknown")` instead of the expected exit-code result. | **Open outside this scanner increment.** Preserve the failure and separately diagnose with deterministic test-first evidence. A status-read/active-lock-observation ordering race is a plausible source hypothesis, not a confirmed root cause. No runtime fix was made or claimed here. |

The failing command path does not invoke the checkpoint scanner, and the runtime implementation/test are unchanged by this two-file commit. All new scanner tests passed twice on Ubuntu before that failure. There is no demonstrated scanner-specific P1/P2 blocker. This supports narrow scanner acceptance; it does not justify suppressing the runtime observation, calling it flaky, or claiming the suspected race is resolved.

## Actual runs and failure preservation

Inspected `git show` and compared the scoped files against the exact commit: no differences from reviewed source. Read original JSON and raw logs for both runs:

- [39083477158](https://github.com/nessalabs/workspace-sdk/actions/runs/39083477158): **failed** overall; Ubuntu failed in the runtime case above, macOS completed successfully. This remains failed evidence. Individual log names show all seven new scanner tests passing under both feature selections on both operating systems.
- [39083692263](https://github.com/nessalabs/workspace-sdk/actions/runs/39083692263): **successful exact-source revalidation** on Ubuntu 24.04 and macOS 15. Both complete `scripts/quality-gate.sh`; both checkout logs identify the reviewed SHA. No source/test change or suppression separates the two runs.

The gate runs formatting, Clippy with warnings denied, feature-selected builds/tests, rustdoc with warnings denied, supervisor build, and explicit managed/PTY qualification. In the successful revalidation, independently counted passing names establish five integration and two private tests twice per OS: 28 new executions. The saved test summary agrees. Reviewer commands only inspected source/evidence and validated this document; no local build or test execution was performed.

Checked `docs/evidence/checkpoint-scanner-2026-10-17/qualification.json` against both original metadata/log pairs. Both job arrays and conclusions match exactly, and both runs' checkout logs match the source SHA. The raw hashes match:

| Run | Raw log SHA-256 |
| --- | --- |
| Failed 39083477158 | `ab3a362caf812f0ac77e7162a3c289eb13b9b928a1bc668c9fcb707e572fb4d8` |
| Successful 39083692263 | `fa96042a3b075f63b20bbd7206722ea48d7e1a1995054a1339dbf233da74ac0e` |

The evidence summary accurately retains the original failure and its unresolved disposition. Revalidation success does not prove flakiness or repair the suspected runtime race.

## Performance evidence and limits

Independently parsed the two complete performance JSON objects per OS from the successful raw log, removing only runner/step/timestamp prefixes. Both full arrays exactly match the evidence summary. Each OS has three managed and fifteen PTY samples with debug assertions enabled; Linux used x86_64 and macOS aarch64.

| Environment | Managed cancellation, ms | PTY cancellation, ms | Retained managed / PTY bytes |
| --- | --- | --- | --- |
| Ubuntu 24.04 | 51.81–51.99 | 51.66–63.83 | 32,768 / 131,072 |
| macOS 15 | 76.41–200.34 | 62.69–220.49 | 32,768 / 131,072 |

Retention values are consistent across their respective samples. These qualify existing runtime scenarios; they do not establish scanner allocation/RSS, throughput, deep-directory descriptor behavior, a speedup, or a statistical regression baseline. Custom scanner bounds are exercised through local capture/archive APIs, not reconfigured target-native defaults. Source/Git writers must remain quiescent; no new snapshot atomicity, live SSH/Boat, offsite or power-loss qualification follows. This independent quality decision does not replace the separate adversarial review required by `AGENTS.md`.
