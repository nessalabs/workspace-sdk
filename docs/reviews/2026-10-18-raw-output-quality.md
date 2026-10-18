# Retained byte output: independent quality review

Decision: **PRECLOUD QUALITY PASS** for the frozen source and local evidence. No unresolved P1/P2 finding. Final qualification requires the full exact-candidate cloud gate and separate independent adversarial review; neither is implied by this preliminary decision.

I independently inspected the application/domain/backend/wire/paging changes and native tests, and verified all eleven files against the frozen source manifest (SHA256 `662682014160250b024f2a923ee997eb076a8a7c8363fc99545681a720c3da35`). The public qualification source map agrees. No build, test or mutation probe was run by this reviewer.

`OutputBytesPage` remains a domain result without serialization and redacts payloads in Debug. Optional backend support refuses unsupported byte reads rather than reconstructing bytes from text. Managed reads validate the workspace-qualified run and requested stream budgets. Base64 encoding belongs to the supervisor wire DTO, whose Debug is also redacted. Existing text output still applies `from_utf8_lossy` to the same slices with unchanged byte cursors, budget split, gap and completion rules. The added allocation does not change its decoding semantics.

The actual native cases exercise every byte value including NUL and invalid UTF-8, multibyte text split at one-byte boundaries, independent/asymmetric cursors, rereads, reconnection, terminal drainage, retained tails/gaps, invalid limits and future cursors, and wrong-workspace rejection. Wire tests verify binary round trips, malformed base64 refusal, payload redaction, maximum-page response size and refusal of text fallback. The two-stream combined page bound remains 2..=262144 bytes.

Actual local evidence: 157 tests passed (143 library, 2 expected-head, 2 native byte-output, 10 SDK workflow); the native byte cases took 1.56s. After semantic probes, the restored supervisor was rebuilt, then both native byte cases passed again in 2.01s. All-feature/all-target Clippy with `-D warnings` passed in 9.39s, strict all-feature documentation passed in 3.91s, and formatting passed. Successful bounded-run reports have exit 0, no cleanup error and the owned process group absent. The initial focused attempt was stopped by the 600MiB disk floor (SIGTERM/-15); it is not a passing result. Its evidence was retained before the later successful run.

Three probes each rebuilt the mutated supervisor before exercising the native test, then produced runtime failures with exit101 rather than compilation failures. Lossy conversion is caught by the managed per-stream budget check; suppressing gaps breaks the explicit gap assertion; early completion breaks the unread-output assertion. Source hashes match after restoration, and the restored supervisor rebuild prevents stale mutated binaries from qualifying the final tests.

Local raw log hashes:

| Check | SHA256 |
| --- | --- |
| second | 911a557272b0997289f9afa4d394d92ea9981ff1d39735f93586532f3de7fc25 |
| restored-build | 07672c7b80a0510cdff7dd3b56a9ca22f72e4539532573d0be0c6a7801914b17 |
| restored | f9d4a7e218609219f6f04a66ba4de76e72c77c3938f38cf1fbd67eadf40d1fed |
| clippy | 36a2771118089d4f0e74b1b6fa575f3e53429307b1917bb2d55ca583b34e6308 |
| docs | a9f14bfc99ae40f797e9bda0fd2b623cd88b5a068529e9cbff4b073553d20cb6 |
| fmt | e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 |

The qualification accurately limits this change to retained byte reads. These logs are bounded retention, not backpressured pipes; consumers must treat a gap as incomplete protocol output. There is no new stdin stream, input EOF, child environment isolation, duplex transport, conversation migration or live remote-provider qualification. The wire tests are not evidence of actual remote transport behavior. Existing providers and older supervisors may refuse the new optional operation, and must never fall back to lossy text. No new performance baseline or latency improvement is claimed from these functional checks.
