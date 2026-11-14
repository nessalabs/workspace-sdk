# Named uses (`Uses`) and Machines built on them: reviews

Change: `src/application/uses.rs` adds `Uses<K, S>`, `NamedUse` and `UseTurn`.
`Machines` keeps its machine handles behind each machine's turn. `tests/uses.rs`
is new. The consumer is Nessa, which keys `Uses` by its own conversation ids
for the Boat VMs it records itself, because `Machines` serves only machines
this SDK saved.

## First round (commit 128e07e)

The quality review and the adversarial review were run independently.

| # | Severity | Finding | Fix |
| --- | --- | --- | --- |
| 1 | P2 (both) | `Uses::hold` took a use without the turn, so a rest that had already decided to stop the thing could stop it under the new use. | `Uses::hold` is gone. A first use is taken only under the turn (`UseTurn::hold`). A further use extends a live one (`NamedUse::also`), and the thing cannot rest while that live use exists. The types now enforce this. |
| 2 | P2 (both) | Entries were never removed, `released` added entries, and `idle()` locked every entry while holding the map lock. | Round 1: `Uses::rested(key, turn)` forgot the entry once no use, turn holder or waiter referred to it (replaced in round 2). Reads (`released`, `names`, `in_use`) add nothing. `idle()` copies the entries out first and checks them after letting the map go. `len()` reports how many entries are kept. |
| 3 | P2 (quality) | Machines' awake handle lived in a second map, guarded only by a comment. | The caller's state `S` now lives inside the turn's mutex and is reached only through `UseTurn` (`Deref`/`DerefMut`). Machines uses `S = Option<OwnedWorkspace>`, and the second map and its lock are gone. |
| 4 | P3 | The due check was written twice, and `lock` was duplicated. | Both now live on `Holders`, and there is one `lock` helper. |
| 5 | P3 | `Use` and `Turn` were too generic at the crate root, and `uses()` returned names. | Renamed to `NamedUse`, `UseTurn` and `names()`. |
| 6 | P3 | `released()` re-checked after a wake-up and could wait forever while uses kept coming and going. | It now answers at the first moment the last use ends after it looked. The doc says a use taken after that moment is the caller's to see. |
| 7 | P3 | The tests missed the race windows. | Added tests for: a use taken between `idle()` and the turn; `released` polled once, the last use dropped before the next poll, then awaited; a rested entry kept while a use or an awaited turn still refers to it, together with its state. |
| 8 | P3 | Debug output, cancellation docs, and the "counted" wording in the module doc. | Added `Debug` impls. The docs now say a turn wait can be cancelled at any point. The module doc says "named". |

**Behaviour change in Machines, kept on purpose.** Before this change, a successful rest kept the old idle start time. A wake that failed right after a rest was therefore stopped on the next pass. Now `rested` clears the start time, so such a failed wake starts a fresh idle period and is stopped one idle time later. This matches "not due again until used or woken". The provider's own lifetime still backs it up. No Machines test covers the failed-wake path yet, because it needs a provider that fails `ensure_ready` after a rest. That remains open as a test gap.

**Limits that remain.**
- An entry used but never rested stays until it is rested. The bound is the number of things the caller has used and not yet rested.
- Pruning reads `Arc::strong_count` on the entry's own internals, under the map lock. Callers never read reference counts.

## Second round (commit da19657)

The quality review found one P2 and two P3s. The adversarial review found no P1 or P2, and three P3s.

| # | Severity | Finding | Fix |
| --- | --- | --- | --- |
| 9 | P2 (quality), P3 (adversarial) | `Uses::rested(key, turn)` took a key and a turn that could disagree. A wrong key could forget another thing's entry and its state. | The turn now carries its key and a borrow of its `Uses`. The call is `UseTurn::rested(self)`, so it cannot name another key. |
| 10 | P3 (both) | An entry whose turn was taken but never used or woken, such as one where `owned_workspace` failed, was never forgotten. `try_turn` recreated an entry that an overlapping rest pass had just forgotten. | Every turn that ends checks its own entry and forgets it when nothing refers to it and it has nothing to wait for (no use, no other turn, no idle time running). `try_turn` never creates an entry. |
| 11 | P3 (quality) | `idle()`'s brief copy of each entry could make an overlapping forget skip. | `idle()` now lists weak references, which never keep an entry. |
| 12 | P3 (adversarial) | The `released` test did not reach the window between the check and the wait inside one poll. | The test is renamed to what it covers. A multi-thread test now ends the last use on another thread 2,000 times while `released` waits, each round with a 5 s bound. |

Also added: `a_turn_that_ends_with_nothing_to_wait_for_forgets_the_thing`.

**Bound now.** An entry is kept while its thing is in use, has a turn held or awaited, or is idle and not yet rested. A caller that never rests an idle thing keeps its entry until it does.
