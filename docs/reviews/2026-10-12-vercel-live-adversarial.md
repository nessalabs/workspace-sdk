# Vercel Linux PTY verification: adversarial review

Reviewer: independent `retained_eof_adversarial` agent. Scope: execution of the
existing SDK PTY integration on a real Vercel Linux sandbox, plus supervisor
lifecycle verification across separate completed remote requests. The root
agent owns sandbox creation, uploads, builds, commands, and cleanup. This
reviewer performs no remote mutation or production edit.

This is not verification of a Boat or SSH transport, and there is no production
Vercel provider adapter in the reviewed scope. Running LocalProvider tests in a
Vercel VM establishes Linux process behavior there; separate supervisor RPCs
must establish independence from remote request lifetimes.

## Required evidence

- Pin SDK and native dependency source revisions, record Linux architecture,
  kernel and toolchain, and compare transferred source hashes. Upload an explicit
  source allowlist, excluding Git metadata, local credentials, environment files,
  build directories, and unrelated local artifacts. Use synthetic challenge
  values instead of account credentials.
- Run existing native acceptance and adversarial suites without weakened
  assertions, plus relevant mechanical and performance checks. Preserve exact
  commands, process exit status, complete test counts, and raw samples.
- Complete the launch RPC, then use a later independent RPC to observe the same
  owner and workload alive. Same-key launch must preserve identity and one launch
  count; output cursors must continue without accidental replay.
- Independently observe real terminal and foreground status, child input
  acknowledgement with echo disabled, and actual child-observed resize. Accepted
  input or a successful resize response alone does not establish child behavior.
- Reject wrong workspace and nonexistent run identities before delivery. Do not
  relabel a missing or failed remote response as proof that no input was sent.
- Confirm cancellation through later state/output observation and independent
  process evidence. Then confirm owned workspace removal and provider sandbox
  stopped state. Preserve unknown cleanup rather than claiming success on timeout.

The supervisor CLI serializes a protocol Failure while normally exiting zero;
the runner must inspect the response envelope as well as command exit status.
Existing fixture executables have a 30-second unconditional watchdog and the
test host uses 20-second launch timeouts. Multi-RPC evidence must use a separately
bounded synthetic workload suitable for that timing, not silently change the
production code or existing test assertions.

Decision: **PENDING** until the exact runner, transferred source, observed remote
results, and cleanup receipts are available for independent review.

## Probe source review

Read the independently authored `examples/pty_live_probe.rs`. Its positive-path
oracle checks three terminal descriptors, foreground process group, exact
transcript/replay/cursors, child-observed input and 120x40 dimensions, saved
workload boot/start identity, one launch, cancellation 130, settled EOF, and
workload absence or zombie state. The synthetic input value is not echoed or
included in saved transcripts. Private state uses an owned 0700 root, 0600
files, an exclusive controller lock, and atomic fsynced saves. Source review
does not substitute for the requested separate completed RPCs.

**P2, closed after source re-review (also found by the independent quality reviewer):** a partial start
can retain phase `created`, and reconnect may deliver input before saving the
updated transcript. The original cleanup path rejected these states or the
additional legitimate output before destruction. Recovery must attempt bounded
owned cancellation/destruction while preserving the earlier failed oracle as a
failure. Provider VM termination remains the final fallback.

Requested evidence improvements before running: save and compare the supervisor
owner identity across requests, observe owner exit or active-lock release before
destruction, and independently confirm project/control roots absent after a
Completed deletion receipt. Root reports an exact public source clone at
`e3465f9afa71cac37dd23326651c8c6ddf939f45`; no local private archive upload is
needed. The original-source Linux mechanical run is distinct from the added
probe's compilation and execution evidence.

The updated probe bounds cancellation and verification separately, captures
verification errors, panics and timeout, and still attempts owned destruction.
It independently checks removed roots and records `cleanup_failed_verification`
with a nonzero result if any earlier oracle failed. Fresh unique 0600 temporary
files prevent a stale pending save from obstructing cleanup. It now records the
supervisor boot/PID/start identity, checks continuity on reconnect, and confirms
owner death or independent active-lock acquisition before destruction. Exact
replay, child observations, and cancellation requirements are unchanged.
**Probe source review: PASS.** Actual remote execution, complete gates, and
provider stop confirmation remain pending.

The first Linux run failed the owner-denied PATH-shadow case while other PTY
cases passed. The initial effective-UID-0 hypothesis was ruled out: root reports
the actual UID is 1000, but the default VM process has capability mask
`000001ffffffffff`, including DAC override. The diagnostic reproduced effective
access success for 0010 and 0755 files and EACCES for 0600. Thus the 0010 fixture's
ordinary owner-permission assumption does not hold under the supplied privilege
set. This is not evidence of incorrect effective-ID lookup.

The unchanged gate will rerun after removing bounding, inheritable and ambient
capabilities and enabling no-new-privileges. Require recorded zero inheritable,
permitted, effective, bounding and ambient capability masks, no-new-privileges,
and 0010 effective-access rejection in that exact environment. Preserve the
initial failed run. No test exclusion, weakened assertion, or SDK implementation
change is authorized by this diagnostic. Final passing claims must explicitly
qualify the capability-free Vercel Linux environment rather than the platform's
default privileged command environment.

## Final independent artifact verification

**PASS** for capability-free `LocalProvider` and supervisor execution inside the
Vercel Linux VM, including ownership across separate remote commands. No
unresolved P1/P2 finding remains in this qualification scope. This decision does
not certify Boat, SSH, a new Vercel provider adapter, real account authentication,
or successful operation under the VM's original capability set.

Independently inspected the complete saved artifacts in `work/live-vercel`:

- All 281 tracked-source SHA-256 entries in `remote/pty-environment.json` match
  Git blobs at `e3465f9afa71cac37dd23326651c8c6ddf939f45`. The reviewed local probe
  matches the remote source hash
  `1b83890199cdfa5e75f1473bc000f4ffedb04adcea5bafed41c248ea5129d734`.
  Environment evidence records Linux 6.18.49 x86_64, Rust 1.98.1, euid 1000,
  all five capability masks zero, and no-new-privileges enabled.
- The 31769-byte retrieved archive hashes to
  `ec87e5a84e8825e73d0fb864df509c8bbcdae5d40bbf163a57462aaab11f1295`, matching
  its remote creation receipt. All 13 regular-file members match the extracted
  files byte-for-byte. The archive contains the explicit evidence file list,
  not a local workspace or credential-directory upload.
- The initial unchanged gate's exit 101 and 15/16 adversarial outcome remain
  preserved. The zero-capability permission probe establishes euid 1000,
  effective-access EACCES for mode 0010, and five zero capability sets.
  `sdk-capfree-gate.log` and `sdk-probe-gate.log` both reach the full mechanical
  success marker, with exit-zero command receipts. Formatting, Clippy, both
  feature configurations, rustdoc, supervisor build, and both performance
  qualifications pass; each feature configuration includes four native and
  sixteen adversarial cases. The latter gate includes the added probe, followed
  by a successful explicit example build.
- Final Linux performance reports contain three managed-run samples and fifteen
  PTY samples. Every PTY retains exactly 131072 bytes. The probe-inclusive gate's
  managed-run qualification takes 0.46 seconds and PTY qualification 5.28 seconds.
  The latter report timestamp is 1791858889, Linux/x86_64 with debug assertions
  enabled. These are observed smoke measurements, not a statistical baseline.
- Three distinct completed command receipts establish start, reconnect, cleanup,
  with exit zero and untruncated tool results. Controller PIDs differ; later
  wrappers independently verify the previous controller absent or zombie.
  Saved supervisor and workload boot/PID/start-tick identities remain equal
  across phases. Exact stdout cursors are 40, 67, 67 with empty stderr. The exact
  transcript establishes real terminal/foreground checks, initial 80x24 size,
  child-consumed input, and child-observed 120x40 resize without echoing the
  synthetic challenge. The cleanup phase's reviewed assertions establish
  cancellation 130, stable EOF, original workload stopped, owner stopped or
  active-lock release, and absent project/control roots after deletion.
- A separate lagging-state recovery run intentionally returns probe exit 1 and
  `cleanup_failed_verification`. Its wrapper independently confirms project and
  control roots absent. The saved error confirms the incomplete-phase oracle
  failed; it was not rewritten as positive qualification. This behaviorally
  closes the partial-cleanup finding.
- `stop-receipt.json` confirms both sandbox and session stopped, with persistence
  disabled. A separate `stopped-state.json` API read confirms the same session
  stopped and no routes. Cleanup is therefore confirmed, not inferred from a
  timeout or expired lease.

The original nonzero capability-mask diagnostic was reported by the root;
the independently saved proof used for acceptance is the explicit zero-capability
environment and permission probe. Preserve that distinction if the earlier
diagnostic receipt is unavailable. No assertion was weakened, no production
source was modified for the remote run, and this reviewer performed no builds
or VM mutations. The separate quality-review decision remains required alongside
this adversarial acceptance and the passing mechanical gates.

## Final publication prose and normalized evidence review

**PASS.** Reviewed the new evidence README section, the integration guide's
revised limits, `docs/evidence/vercel-linux-pty-live.json`, and the published PTY
performance samples against the retained raw results. Scope is explicitly
capability-free LocalProvider/BackendRuntimeHost execution in one Vercel Linux
VM, including separate remote-controller lifetimes. No Boat, SSH/container,
production Vercel adapter, real authentication, WAN-latency, or statistical
baseline claim was introduced.

Every published phase field matches its raw phase record. Source-file, test,
and performance counts are accurate; the published performance JSON is
byte-identical to the raw numeric report. The local full-gate log also reaches
successful completion, supporting that separately labeled gate result. The
source-manifest digest independently matches sorted-key compact JSON of the
281-entry manifest:
`63145115e59fb36a49a23e566c488f1b462d4be2947c7eb02395f17e9671561e`.
The environment-record digest is verified separately against the complete raw
environment file; neither digest is mislabeled as the other.

The public JSON omits sandbox, project, team, boot, workspace-reference and
process-reference identifiers and local-account paths. It retains only the
numeric process observations necessary for continuity evidence, source/binary
hashes, normalized environment, results and measurements. No credential marker
or synthetic private-input value appears in the published evidence.

`work/live-vercel/diagnostic-observations.json` now preserves the original
capability and three-mode effective-access observations as an explicitly
transcribed operator record. Its provenance states that historical API logs
returned 410 after the stopped sandbox; it does not claim to be a re-fetched
provider receipt. Acceptance remains anchored to independently saved
capability-free diagnostics, source identity, complete gates, phase receipts,
and stopped-state read. This clarification closes the evidence-recording note
without inventing a new observation. No publication blocker remains.
