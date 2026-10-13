# Auditable Vercel Linux qualification

[index.json](index.json) inventories the committed artifacts and records both original
and published SHA-256 values. Cloud identifiers, UUID references, host scopes and
the private hostname use consistent aliases; numeric measurements, outcomes and
source/binary digests remain intact. This permits verification from a clean checkout
without access to ignored `work/` files. Original unsanitized receipts are retained
privately. The operator-transcribed privileged diagnostics are explicitly identified
in the index; they are not represented as independently downloaded raw receipts.

The historical run compiled SDK commit
`e3465f9afa71cac37dd23326651c8c6ddf939f45` plus
[original-probe-source.rs](original-probe-source.rs), SHA-256
`1b83890199cdfa5e75f1473bc000f4ffedb04adcea5bafed41c248ea5129d734`.
The current [probe](../../../examples/pty_live_probe.rs) additionally rolls back
workspace creation if its first state save fails. That later fix is covered by
`tests/pty_live_probe.rs` and the updated local gate (with hosted CI required
before merging this revision); its bytes must not be
attributed to the earlier live run.

## Fresh-run setup

Use an authenticated Vercel Sandbox client/connector and your own team/project.
Create a **fresh**, nonpersistent sandbox with the following request body, replacing
only the project ID and unique name. Retain its returned session ID for all calls:

```json
{
  "projectId": "YOUR_PROJECT_ID",
  "name": "YOUR_FRESH_QUALIFICATION_NAME",
  "persistent": false,
  "timeout": 1800000,
  "resources": { "vcpus": 2 },
  "architecture": "amd64",
  "image": "vercel/sandbox/universal@sha256:c77f7436b9bc0a8b01ef7b09dbcea2de6a18a9fe2aafa89996d2da193da72cf0",
  "source": {
    "type": "git",
    "url": "https://github.com/nessalabs/workspace-sdk.git",
    "revision": "e3465f9afa71cac37dd23326651c8c6ddf939f45",
    "depth": 1
  }
}
```

In that VM, install `build-essential` and `pkg-config` with apt as root. Install
Rust 1.98.1, rustfmt and Clippy as the normal command user, with
`CARGO_HOME=/vercel/sdk-cargo` and `RUSTUP_HOME=/vercel/sdk-rustup`:

```sh
curl --fail --silent --show-error https://sh.rustup.rs -o /vercel/sdk-rustup-install.sh
CARGO_HOME=/vercel/sdk-cargo RUSTUP_HOME=/vercel/sdk-rustup \
  sh /vercel/sdk-rustup-install.sh -y --no-modify-path --profile minimal \
  --default-toolchain 1.98.1 --component rustfmt --component clippy
```

The observed checkout was `/vercel/workspace-sdk`; verify `git rev-parse HEAD`
and the actual checkout path before execution. Copy the archived probe source to
`/vercel/workspace-sdk/examples/pty_live_probe.rs` through the authenticated file
upload API to replay the historical version. Use the current probe instead when
qualifying its later fixes, and record its different source hash explicitly.

## Gate and build

Run this as one completed remote command, with the stated cwd and environment:

```sh
cd /vercel/workspace-sdk
export CARGO_HOME=/vercel/sdk-cargo RUSTUP_HOME=/vercel/sdk-rustup
export PATH=/vercel/sdk-cargo/bin:$PATH
setpriv --bounding-set=-all --inh-caps=-all --ambient-caps=-all --no-new-privs -- \
  bash -lc 'set -eu
    export PATH=/vercel/sdk-cargo/bin:$PATH
    grep -E "^Uid:|^Cap|^NoNewPrivs:" /proc/self/status
    git diff --exit-code
    scripts/quality-gate.sh
    cargo build --locked --example pty_live_probe'
```

Require UID 1000, all five capability sets zero, `NoNewPrivs=1`, complete mechanical
success including both performance targets, and command exit zero. This restores
the owner-denial test precondition; do not skip the permission assertion. The
committed `sdk-full-gate.log` preserves the initial failure with default capability
bypass; `sdk-capfree-gate.log` and `sdk-probe-gate.log` preserve successful reruns.
The historical gate predates the later explicit per-feature probe-build commands.

## Three separate completed remote commands

Submit each block in its own remote `runCommand`/`run_session_command` invocation.
Do not combine the three into one controller. Wait for each command's completed
exit-zero receipt before starting the next. Every invocation uses the same
`setpriv` envelope. The saved output supplies prior-controller identity; no
historical PID is hardcoded into these commands.

Start:

```sh
setpriv --bounding-set=-all --inh-caps=-all --ambient-caps=-all --no-new-privs -- \
  bash -lc 'set -eu
    grep -E "^Cap|^NoNewPrivs:" /proc/self/status
    /vercel/workspace-sdk/target/debug/examples/pty_live_probe start \
      /vercel/pty-qualification-positive \
      /vercel/workspace-sdk/target/debug/workspace-supervisor \
      > /vercel/pty-phase-start.json
    cat /vercel/pty-phase-start.json'
```

Reconnect, input and resize:

```sh
setpriv --bounding-set=-all --inh-caps=-all --ambient-caps=-all --no-new-privs -- \
  bash -lc 'set -eu
    python3 -c "import json; from pathlib import Path; s=json.load(open(\"/vercel/pty-phase-start.json\")); p=Path(\"/proc\")/str(s[\"controller_pid\"])/\"stat\"; assert not p.exists() or p.read_text().rsplit(\") \",1)[1].split()[0]==\"Z\""
    grep -E "^Cap|^NoNewPrivs:" /proc/self/status
    /vercel/workspace-sdk/target/debug/examples/pty_live_probe reconnect \
      /vercel/pty-qualification-positive \
      /vercel/workspace-sdk/target/debug/workspace-supervisor \
      > /vercel/pty-phase-reconnect.json
    cat /vercel/pty-phase-reconnect.json'
```

Cancel and clean:

```sh
setpriv --bounding-set=-all --inh-caps=-all --ambient-caps=-all --no-new-privs -- \
  bash -lc 'set -eu
    python3 -c "import json; from pathlib import Path; s=json.load(open(\"/vercel/pty-phase-reconnect.json\")); p=Path(\"/proc\")/str(s[\"controller_pid\"])/\"stat\"; assert not p.exists() or p.read_text().rsplit(\") \",1)[1].split()[0]==\"Z\""
    grep -E "^Cap|^NoNewPrivs:" /proc/self/status
    /vercel/workspace-sdk/target/debug/examples/pty_live_probe cleanup \
      /vercel/pty-qualification-positive \
      /vercel/workspace-sdk/target/debug/workspace-supervisor \
      > /vercel/pty-phase-cleanup.json
    cat /vercel/pty-phase-cleanup.json'
```

The probe asserts the same owner/workload identities, one launch, exact replay and
cursors, child-observed input and 120×40 dimensions, cancellation code 130, stable
EOF, stopped/released ownership and independently absent project/control paths.
The expected transcript and outcomes are preserved in the three `pty-phase-*.json`
files; `phase-*.rpc.json` preserves the exact historical argument arrays and
completed receipts. Recovery receipt files preserve the separate deliberate
lagging-state case: verification exits 1 while owned paths are removed. Its exact
injection and validation commands are in `recovery-start.rpc.json` and
`recovery-cleanup.rpc.json`.

Download all logs, phase outputs and both performance JSON files before shutdown.
In a `finally` block, call Sandbox.stop()/stop_session for this exact newly created
sandbox, including after a failed phase. Independently read its state and require
`stopped`; a stop request alone is insufficient. `stop-receipt.rpc.json` and
`stopped-state.rpc.json` preserve both observations from the completed run.

This qualifies SDK LocalProvider execution inside a capability-constrained Vercel
Linux VM. Boat/SSH/container transport, a production Vercel adapter and real agent
authentication remain outside the observed run.
