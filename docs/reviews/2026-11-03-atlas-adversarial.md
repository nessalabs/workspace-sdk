# Workspace SDK independent review

## Docs-only task pass

Before looking at source/models, followed README links to open-identity, directory-baseline, edit-review, and handoff.

1. **Open build-17 timed out:** open-identity's readiness example says provider creation may already have happened and the valid descriptor is saved before readiness. Retry exactly the same provider/request under build-17 to reconnect, not a changed request to force recreation. Provider-call/save failure instead requires provider evidence because reservation alone proves neither creation nor rollback. Supported by "Retry the same provider/request under build-17; the saved reference selects reconnect."
2. **Directory page two fails:** directory-baseline's example says retain the prior complete view as stale and retry the entire poll; never merge page one's partial candidate. A fresh observer deliberately resets. Supported by "The poll returns Conflict and keeps the old complete baseline."
3. **Another candidate staged after review:** edit-review says successful stage invalidates prior review; obtain the new revision and review again. Interrupted apply must use engine recovery, not stage over it; cancelling the waiter doesn't prove filesystem work stopped. Supported by "Applying the old review must fail even if the caller still has its reference."
4. **Source run Lost:** handoff says Lost is not terminal and cannot authorize destination launch. Reconnect to establish terminal state, quiesce other writers and hand off to a different target with explicit continuation. Retain operation ID if caller stops waiting because operation may still finish. Supported by "It cannot use that uncertainty as permission to start the same work elsewhere."

All four sampled conceptual decisions were answerable without implementation lookup. These are reasoning tasks, not a claim that the reader can implement all APIs without further usage examples. Current pages expose key uncertainty in prose and selected inspectors; no material contradiction found in the sampled pages on this pass.

## Source/model adversarial pass

Reviewed the four TLA specs/configs, persisted controls, canonical composition runner, and source mappings in workspace_sdk.rs, directory_observer.rs, edit/session.rs and sync/sync_service.rs at recorded source 867a4c15e97e025a1066b3c0cd133ffa81f374f1.

### W1 — P2: directory model overstates completeness checking

The page says it checks that published candidates contain both pages with one revision. ConsistentPublication only records first=second. Publish assigns pages'=2 itself; no property independently records how many pages actually arrived. The second-page state and publication-count label can therefore be conflated. The existing mutation challenges revision agreement and abort retention, not completeness.

Prefer explicit receipt of both pages, a captured accepted-page count/final-page fact, a publication completeness invariant and a mutation bypassing the completion guard. Alternatively narrow the formal claim and explicitly state completeness is source-reviewed but not independently checked. Rerun affected baselines/controls after the change.

### W2 — P3: state restoration guard coverage explicitly

Handoff accurately checks source terminal before capture, restore and destination start in Rust. The model includes all three guards, but checks only capturedSafe and startedSafe; the controls likewise omit restore. Owner model prose is already mostly scoped to capture/start. Either add a RestoreAfterStop property/control or explicitly exclude restoration safety from model claims. Do not imply every source-stop check is independently challenged.

### Source and model checks without actionable defect

- OpenIdentity keeps the first reservation owner independent from later requests; reservation mutation reaches a conflicting request and descriptor mutation reaches mismatched saved identity. The model excludes provider idempotency, lost replies, hashing and disk durability explicitly. Source saves a valid descriptor before ensure_ready, exactly as the failure recipe says.
- DirectoryObserver builds a candidate locally, validates revisions/cursors/duplicates/bounds, and replaces baseline only after the loop; cancellation before that preserves baseline. The docs' stale-view retry advice matches the source.
- EditReview retains an offered old review while successful staging clears latest and increments generation. Apply captures the accepted generation and quiescence assertion independently, so the stale/current guards and quiet mutation are meaningful. Quiescence is correctly described as a caller obligation, not an OS lock. Source check_revision/check_review/ensure_editable preserve those restrictions.
- Handoff source Lost is nonterminal; Capture and Start capture the contemporaneous state rather than deriving safety from a final phase. Allowing observation to change after capture makes the later guard nontrivial. Rust checks receipt identity, preserves deterministic destination run identity, returns already Started receipts without repeating effects, and rechecks source at each new effect boundary. The docs correctly distinguish caller cancellation from operation rollback.
- Scripts delegate catalog/checker parsing and writing to canonical Nessa Docs, save baseline records separately from mutants and refuse missing controls. Current receipts are provisional until the owner's final rerun; I did not count unexecuted mechanical suites as passed.

## Re-review and disposition

W1 resolved: DirectoryBaseline now advances pages only through ReadPage, captures publishedPages from that independent count, and checks CompletePublication. The new publish-before-final-page control permits publication after one received page; unchanged property should catch it. Owner wording accurately explains this distinction.

W2 resolved: Handoff now records restoredSafe from the observed source at Restore, checks RestoreAfterStop, and has a restore-lost control. Source can independently become Lost after capture so this control is reachable.

No unresolved P1/P2 findings in this reviewed documentation/model scope. Acceptance still depends on the author's final mechanical/baseline/control results and the separate code-quality review. I reviewed the changed models and guards; I did not run or invent those final receipts. This targeted review is not a provider integration proof or a complete implementation audit.
