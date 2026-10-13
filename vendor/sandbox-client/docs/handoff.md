# Durable handoff receipts

A handoff receipt is a durable progress record: **checkpoint captured → workspace
restored → continuation started**. A checkpoint reference says where your
application saved the workspace and optionally records its digest.

The SDK saves that record. Your application captures and copies files, verifies
the result, and launches or reconnects to work. It also stops or otherwise
quiesces the source before capture so the checkpoint is consistent. The SDK does
not move a running process or implement a checkpoint format.

## Select storage and application actions

`HandoffJournal<S>` accepts any `ReceiptStore`. The included `FileReceiptStore`
uses an application-selected directory on Unix. Use a database-backed
`ReceiptStore` for controllers on different machines. This choice is separate
from `BoatClientConfig`, which configures HTTP calls.

```rust,no_run
use boat_client::handoff::{FileReceiptStore, HandoffJournal, HandoffPlan, HandoffResult};

#[tokio::main]
async fn main() -> HandoffResult<()> {
    let store = FileReceiptStore::new("./state/box-handoffs")?
        .with_max_receipt_bytes(1024 * 1024);
    let journal = HandoffJournal::new(store);
    let receipt = journal.create("job-2026-001", HandoffPlan {
        source_sandbox_id: "bx_23456789".into(),
        target_sandbox_id: "bx_abcdefgh".into(),
        intent: "immutable-manifest-and-continuation-digest".into(),
    }).await?;
    println!("phase={:?}, revision={}", receipt.phase(), receipt.revision());
    Ok(())
}
```

Choose a stable operation ID and reuse it after reconnecting. IDs are 1–96 ASCII
letters, digits, underscores or hyphens. The complete plan is immutable: the same
ID with a different plan returns `PlanMismatch`. Make `intent` identify all
inputs that affect the handoff, including workspace selection, exclusions, and
continuation arguments. Reopening an existing ID returns its current receipt;
it never resets progress.

Use `capture_with`, `restore_with`, and `start_with` to connect your application's
actions. Each helper first commits intent, invokes your callback once, then saves
its result. Capture returns `CheckpointReference`; restore returns `()` after
verification; start returns the remote run ID. Callbacks are never automatically
retried. This example accepts your capture implementation as a callback:

```rust,no_run
use std::future::Future;
use boat_client::handoff::{
    CheckpointReference, HandoffJournal, HandoffReceipt, HandoffResult, ReceiptStore,
};

async fn capture<S, F, Fut>(
    journal: &HandoffJournal<S>,
    prepared: &HandoffReceipt,
    capture_workspace: F,
) -> HandoffResult<HandoffReceipt>
where
    S: ReceiptStore,
    F: FnOnce(HandoffReceipt) -> Fut,
    Fut: Future<Output = HandoffResult<CheckpointReference>>,
{
    journal.capture_with(prepared, capture_workspace).await
}
```

The callback receives the receipt, including the stable operation ID and plan.
Your restore callback also has the checkpoint reference. Use the stable ID to
correlate remote work, and reuse provider idempotency keys where supported.
Persist immutable checkpoint data before confirming capture. A reference or
digest alone does not prove the data exists or matches: your application verifies
that. Do not put API keys, file contents, or expiring signed URLs in receipts.

## Recovery after a crash or ambiguous failure

| Saved phase | Meaning on reload | Application action |
| --- | --- | --- |
| `Prepared` | Capture has not been claimed | Claim capture and capture the quiescent workspace |
| `Capturing` | Capture may have completed | Find and verify the checkpoint, then confirm it |
| `Captured` | Checkpoint reference is saved | Claim restore and transfer/verify the workspace |
| `Restoring` | Restore may have completed partially or fully | Inspect the target; complete or reconcile the restore |
| `Restored` | Restore was confirmed | Claim continuation launch |
| `Starting` | The continuation may already be running | Find the existing run before deciding whether to launch |
| `Started` | The remote run ID is saved | Reconnect to that run; do not launch another |

For example, the target can start successfully just before your controller
crashes. The receipt remains `Starting`. On restart, look up the remote run using
your application's correlation mechanism and record `Started` with that run ID.
The SDK's receipt alone cannot determine whether the remote launch happened.

```rust,no_run
use boat_client::handoff::{
    HandoffJournal, HandoffPhase, HandoffResult, HandoffTransition, ReceiptStore,
};

// Call only after your application has verified this is the existing remote run.
async fn confirm_existing_run<S: ReceiptStore>(
    journal: &HandoffJournal<S>, operation_id: &str, verified_run_id: String,
) -> HandoffResult<()> {
    let receipt = journal.load(operation_id).await?;
    if receipt.phase() == HandoffPhase::Starting {
        journal.advance(&receipt, HandoffTransition::Started {
            run_id: verified_run_id,
        }).await?;
    }
    Ok(())
}
```

`advance` also exposes the individual begin/confirmation transitions when your
application needs to orchestrate them itself. Complete the begin write before
performing the external action. A successful begin grants that invocation one
claim; simply loading an in-flight receipt does not grant a new claim.

If you prove an action did **not** apply, cannot still complete, and any partial
effects have been made safe, use `ReconcileNotApplied` with an audit-evidence
reference to return to the previous confirmed phase. A timeout, missing response,
or an expired local lease is not proof. The SDK records this assertion; it cannot
validate it. It retains the latest such evidence reference, not a full audit log.

## Concurrency, errors, and durability

Every update compares the expected revision atomically. Competing or stale
workers receive `Conflict`; a locally locked receipt returns `Busy`. Reload and
reconcile before proceeding. The SDK does not silently steal an in-flight claim.

After an error or cancellation, a storage write may still have committed.
Canceled file-store writes can finish in their blocking worker. Reload to find
out; never perform an external action based on an unacknowledged begin write.
`HandoffError` distinguishes concurrency errors, plan mismatches, invalid
transitions, missing/corrupt records, size limits, and backend failures. Storage
error formatting hides backend details; deliberate error-source access remains
available for diagnostics.

The file store uses OS locks, same-directory atomic replacement, file sync and
directory sync. Reads take the same lock and finish durability confirmation
before returning a receipt. File I/O runs on Tokio's blocking pool. New
directories/files use `0700`/`0600`; existing directory permissions are unchanged.
The default receipt size limit is 1 MiB and can be configured. Encoded filenames
keep case-distinct IDs separate on case-insensitive filesystems.

These guarantees require an application-owned **local filesystem** supporting
locks, atomic rename and directory sync. Network filesystems and multi-host
controllers need a suitable transactional backend. Custom `ReceiptStore`
implementations must return only committed records and durably perform atomic
compare-and-swap writes. No in-memory backend is presented as durable storage.

Keep receipt records and immutable checkpoint data available for the workflow's
recovery lifetime. Retention, encryption, backup, and eventual cleanup belong to
the selected storage backend and application policy. The file store does not
expire or delete receipts automatically. Crash-left temporary files are ignored;
lock files must remain in place while controllers may use the directory.

Durable receipts support recovery; they do not create an exactly-once guarantee
for non-idempotent remote actions. This follows the distinction between retries
and idempotent operations described in the
[Amazon Builders' Library](https://aws.amazon.com/builders-library/making-retries-safe-with-idempotent-APIs/).
