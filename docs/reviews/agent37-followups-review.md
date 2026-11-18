# Agent37 provider follow-ups: quality and adversarial review

Decision: **PASS** after fixes. Scope: the budget fix carried from #16
(`ready`/`stop` bounded by their deadline) and the #21 follow-ups. Reviewers:
two independent subagents, read only, one quality and one adversarial.

## Findings and disposition

| Sev | Finding | Disposition |
| --- | --- | --- |
| P1 | `decode_reference` re-exported only with `agent37`, so a `space`-only build failed (the gate caught it too) | Fixed: re-exported for `space` or `agent37` |
| P2 | A reaper killed with its runtime left `reaping` set forever, so the private link was never dropped again | Fixed: the slot keeps the reaper's handle and replaces one that has finished; a reaper that ends clears itself under the lock |
| P3 | The `nc -U` probe's `select!` could prefer `wait` over a connection already made | Fixed: `biased`, accept first |
| P3 | Space's version/provider mismatch message is now the shared one | Accepted: nothing matches on the old text |
| P3 | `Region` and `in_region` were API with one value behind it | Fixed: removed; the spec documents that `default` instances run only in `us` |
| P3 | `Size` lost its readable fields | Fixed: `cpu()`, `memory()`, `disk()` |
| P3 | Tunnel module widened only for a test | Fixed: `Tunnel` re-exported under `cfg(test)` |
| P3 | Probe imports, doc, and the uncached failure cost | Fixed: imports at the top, doc says what is remembered |
| P3 | Reaper test proved only "no new tasks" | Fixed: it also checks the one reaper still drops the value |
| P3 | CI job's choices unexplained | Fixed: comments on the account, the copy to `/tmp` and the thread count |
| P3 | Redundant deadline checks inside `ready_by`/`stop_by` | Kept: they give up one poll early rather than sleep past the budget |
