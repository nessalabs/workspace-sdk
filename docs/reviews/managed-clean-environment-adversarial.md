# Managed clean environment: adversarial review

**PRECLOUD ADVERSARIAL PASS. No unresolved P1/P2 finding; final acceptance requires the exact-source full cloud gate.** Independent reviewer: `workspace_handoff_adversarial_review`. Other agents authored production/tests; this reviewer ran no builds or probes and changed only this record.

## Source and boundary assessment

The additive `start_piped_with_clean_environment` method uses the existing managed run claim, private transport and pipe owner. Optional backends refuse unsupported clearing; there is no inherited-mode fallback. Launch request literals and prior modes retain their encoding and behavior.

An absolute executable is enforced by Workspace, managed backend and native admission before claim creation. This closes the design concern that a relative program might use a platform default executable search even without PATH. Native command setup clears inherited variables before removals, public values and finally transient secret values. Thus explicit values can reintroduce removed names, and secrets override public duplicates. Clearing applies only to the workload child. The new operation is exhaustively classified for private payload transport.

The clean-only fingerprint separator precedes the existing piped separator. Exact clean retries reconnect; same-ID clean/inherited/ordinary rebinding conflicts. Prior piped encoding has an independent golden calculation. No extra journal or control endpoint is introduced, and input prefix/Unknown/EOF/ownership semantics remain those of managed pipes.

Three real-native cases compare an inherited-piped child against a clean child under a synthetic runner environment, without process-global environment mutation. They verify sentinel/PATH/HOME absence, explicit removals and precedence, synthetic secret redaction from claims/Debug, exact binary input/output, EOF, reconnect, retry and mode conflicts. The third case observes the new operation in private request stdin, simulates an unsupported supervisor with bounded invocation counts and no fallback, then mutates the payload after API/backend validation to exercise native absolute-path refusal. Shared registered authority and claim cleanup cover the custom-runner workspace.

## Actual local evidence and sensitivity

The initial focused command passed 164 tests: 145 library, three clean-environment, four pipe, two byte-output and ten workflow cases. Its outer bounded metadata records exit zero, no stop or cleanup error, owned process group absent and 32.05869 seconds elapsed.

The three parent-owned native mutations each built the mutated supervisor successfully, then exited 101 through the intended runtime assertion:

- Removing clearing exposed the inherited synthetic sentinel.
- Removing clean-mode identity separation allowed a prohibited inherited-piped claim rebind.
- Reversing explicit environment precedence returned the public value instead of the synthetic secret value.

Actual failure logs agree with `clean-env-probes-proof.json`; these are semantic catches, not compile failures. All nine frozen file hashes match after restoration. Manifest SHA-256: `a957480e42b80acf3e7bf9acde8fa788cc27c8dd488e02cb9970ce60d74ea608`.

A rebuilt restored supervisor passed before rerunning all three new native cases and four retained pipe cases, seven passing in total (groups 1.28 and 1.23 seconds; outer command 10.41360 seconds). All-feature/all-target denied-warning Clippy, strict documentation and formatting also passed. Completed bounded metadata records exit zero/no stop/no cleanup error/group absence for each command. The runner's group observation does not itself establish cleanup of detached native workloads; the tests separately inspect registered claims and ownership.

## Decision and limits

**Precloud PASS:** source, local qualification, mutation sensitivity and exact restoration are consistent. No unresolved P1/P2 finding remains. Full remote qualification is pending.

This is environment construction, not an executable/filesystem sandbox or secret-output sanitizer. Absolute launchers, interpreters and the operating system can introduce variables themselves. No PATH/HOME/locale/loader defaults are reconstructed. Supervisor/transport environments and credentials are unaffected. No agent host adapter, application migration, physical failure, arbitrary external-effects exactly-once guarantee or live-provider qualification is established. Existing input ambiguity, retained-output gaps and cleanup responsibilities remain unchanged. Local timings are observations, not a baseline or speedup claim.
