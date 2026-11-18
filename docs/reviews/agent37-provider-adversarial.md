# Agent37 provider: independent adversarial review

Decision: **ADVERSARIAL PASS**. No unresolved P1/P2 finding; one P2 accepted
as a recorded residual risk, with the reviewer's conditions below.

Scope: as in [the quality review](agent37-provider-quality.md). Reviewer: an
independent subagent, read only, four rounds.

## Findings and disposition

| # | Sev | Finding | Disposition |
| --- | --- | --- | --- |
| P2-1 | P2 | Tunnel host key "pinning" bypassable: every dial used `accept-new`, which learns a key of a new type | Fixed: the master's single learnt key is pinned; later dials use `StrictHostKeyChecking=yes` and that key's `HostKeyAlgorithms` (the first fix set but never used it; caught on re-review) |
| P2-2 | P2 | The workspace-wide private key is written to `/tmp/wsdk-ssh-*/key` and survives a SIGKILL | Residual, accepted (below) |
| P2-3 | P2 | A whole new connection per private request, terminal input included | Fixed: private requests share one link while its master lives |
| P2-4 | P2 | A sleeping instance admitted but not dialable within SSH's wait; exec not retried on codes that never started | Fixed: `ensure`, `connection` and `private` wake first; exec retried only on `try_again` |
| P2-5 | P2 | A lost private request reported as a plain failure | Fixed: reported as "may have run", never resent |
| P2-6 | P2 | An instance still being made judged as missing its digest, and so deletable | Fixed: `ProvisioningUncertain` until it has finished |
| new | P2 | The shared link's keepalives kept an idle instance awake forever | Fixed: `IdleSlot` drops it 120 s after its last use, asked for or not |
| P3 | P3 | Tunnel shutdown race; accept error ends the loop; unbounded codes; duplicate tags; one failed read aborts a wait; plaintext to any host | Fixed |
| P3 | P3 | `nc -U` assumed on the host; open connections hold the instance awake | Documented here: macOS and OpenBSD netcat have `-U`; the link idles out |
| P3 | P3 | One sleeping reaper task per use | Follow-up: a single resettable reaper |

## Residual risk: the key on disk (P2-2)

The file is the workspace's one registered key, valid for **every**
instance of the Agent37 workspace, not a per-connection key as on Boat. It
is mode 0600 in a 0700 directory, so only the same account can read it, and
that account can already read wherever the application keeps the key. It is
removed with the connection; a SIGKILL or abort leaves it, and the shared
private link keeps a copy for as long as the link lives. Revoking it is
deleting the key in Agent37. Follow-up: sweep this account's stale
`/tmp/wsdk-ssh-*` directories at start, then serve the key from an
in-process agent socket so it never touches disk.
