#![cfg(unix)]
mod common;
use boat_client::handoff::*;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "boat-client-handoff-test-{:032x}",
            fastrand::u128(..)
        )))
    }
    fn journal(&self) -> HandoffJournal<FileReceiptStore> {
        HandoffJournal::new(FileReceiptStore::new(&self.0).unwrap())
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn plan() -> HandoffPlan {
    HandoffPlan {
        source_sandbox_id: "bx_23456789".into(),
        target_sandbox_id: "bx_abcdefgh".into(),
        intent: "manifest-and-command-digest".into(),
    }
}
fn checkpoint() -> CheckpointReference {
    CheckpointReference {
        reference: "application-store://checkpoint-1".into(),
        digest: Some("sha256:fixture".into()),
    }
}

#[tokio::test]
async fn restarts_preserve_each_intent_checkpoint_and_run_receipt() {
    let dir = Directory::new();
    let mut receipt = dir.journal().create("handoff-1", plan()).await.unwrap();
    let steps = [
        HandoffTransition::BeginCapture,
        HandoffTransition::Captured(checkpoint()),
        HandoffTransition::BeginRestore,
        HandoffTransition::Restored,
        HandoffTransition::BeginStart,
        HandoffTransition::Started {
            run_id: "run-1".into(),
        },
    ];
    for step in steps {
        receipt = dir.journal().advance(&receipt, step).await.unwrap();
        let reloaded = dir.journal().load("handoff-1").await.unwrap();
        assert_eq!(receipt, reloaded);
        if matches!(
            receipt.phase(),
            HandoffPhase::Capturing | HandoffPhase::Restoring | HandoffPhase::Starting
        ) {
            assert!(reloaded.phase().needs_reconciliation());
        }
    }
    assert_eq!(receipt.revision(), 6);
    assert_eq!(receipt.checkpoint(), Some(&checkpoint()));
    assert_eq!(receipt.run_id(), Some("run-1"));
    assert!(!receipt.phase().needs_reconciliation());
    assert_eq!(
        dir.journal().create("handoff-1", plan()).await.unwrap(),
        receipt
    );
}

#[tokio::test]
async fn stable_id_rejects_different_intent_and_stale_or_invalid_transitions() {
    let dir = Directory::new();
    let journal = dir.journal();
    let original = journal.create("operation", plan()).await.unwrap();
    let mut changed = plan();
    changed.intent = "different-command".into();
    assert!(matches!(
        journal.create("operation", changed).await,
        Err(HandoffError::PlanMismatch)
    ));
    assert!(matches!(
        journal
            .advance(&original, HandoffTransition::BeginStart)
            .await,
        Err(HandoffError::InvalidTransition(_))
    ));
    let capturing = journal
        .advance(&original, HandoffTransition::BeginCapture)
        .await
        .unwrap();
    assert!(matches!(
        journal
            .advance(&original, HandoffTransition::BeginCapture)
            .await,
        Err(HandoffError::Conflict)
    ));
    assert!(matches!(
        journal
            .advance(&capturing, HandoffTransition::BeginCapture)
            .await,
        Err(HandoffError::InvalidTransition(_))
    ));
    assert_eq!(journal.load("operation").await.unwrap(), capturing);
}

#[tokio::test]
async fn concurrent_workers_cannot_both_claim_the_same_step() {
    let dir = Directory::new();
    let receipt = dir.journal().create("operation", plan()).await.unwrap();
    let one = dir.journal();
    let two = dir.journal();
    let (a, b) = tokio::join!(
        one.advance(&receipt, HandoffTransition::BeginCapture),
        two.advance(&receipt, HandoffTransition::BeginCapture)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let failure = match a {
        Err(error) => error,
        Ok(_) => b.unwrap_err(),
    };
    assert!(matches!(
        failure,
        HandoffError::Conflict | HandoffError::Busy
    ));
    assert_eq!(dir.journal().load("operation").await.unwrap().revision(), 1);
}

#[tokio::test]
async fn pending_actions_require_explicit_reconciliation_before_retry() {
    let dir = Directory::new();
    let journal = dir.journal();
    let r = journal.create("operation", plan()).await.unwrap();
    let r = journal
        .advance(&r, HandoffTransition::BeginCapture)
        .await
        .unwrap();
    let r = journal
        .advance(&r, HandoffTransition::Captured(checkpoint()))
        .await
        .unwrap();
    let r = journal
        .advance(&r, HandoffTransition::BeginRestore)
        .await
        .unwrap();
    assert!(matches!(
        journal.advance(&r, HandoffTransition::BeginRestore).await,
        Err(HandoffError::InvalidTransition(_))
    ));
    let r = journal
        .advance(
            &r,
            HandoffTransition::ReconcileNotApplied {
                evidence_reference: "audit://restore-rolled-back-and-old-worker-stopped".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(r.phase(), HandoffPhase::Captured);
    assert!(r.last_reconciliation().unwrap().starts_with("audit://"));
    let r = journal
        .advance(&r, HandoffTransition::BeginRestore)
        .await
        .unwrap();
    let r = journal
        .advance(&r, HandoffTransition::Restored)
        .await
        .unwrap();
    let r = journal
        .advance(&r, HandoffTransition::BeginStart)
        .await
        .unwrap();
    // A timed-out launch may already have succeeded. Reconcile its remote run id,
    // without another BeginStart or another launch.
    let reloaded = dir.journal().load("operation").await.unwrap();
    assert_eq!(r, reloaded);
    let r = journal
        .advance(
            &reloaded,
            HandoffTransition::Started {
                run_id: "existing-remote-run".into(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        journal.advance(&r, HandoffTransition::BeginStart).await,
        Err(HandoffError::InvalidTransition(_))
    ));
}

#[derive(Clone)]
struct LoseAcknowledgement {
    inner: FileReceiptStore,
    fail: Arc<AtomicBool>,
}
impl ReceiptStore for LoseAcknowledgement {
    async fn load(&self, id: &str) -> HandoffResult<Option<HandoffReceipt>> {
        self.inner.load(id).await
    }
    async fn compare_exchange(
        &self,
        receipt: &HandoffReceipt,
        revision: Option<u64>,
    ) -> HandoffResult<()> {
        self.inner.compare_exchange(receipt, revision).await?;
        if self.fail.swap(false, Ordering::SeqCst) {
            return Err(HandoffError::storage(std::io::Error::other(
                "injected lost acknowledgement",
            )));
        }
        Ok(())
    }
}

#[tokio::test]
async fn lost_storage_acknowledgements_reload_committed_state_without_repeating_work() {
    let dir = Directory::new();
    let fail = Arc::new(AtomicBool::new(true));
    let journal = HandoffJournal::new(LoseAcknowledgement {
        inner: FileReceiptStore::new(&dir.0).unwrap(),
        fail: fail.clone(),
    });
    assert!(matches!(
        journal.create("operation", plan()).await,
        Err(HandoffError::Storage(_))
    ));
    let mut r = journal.create("operation", plan()).await.unwrap();
    for transition in [
        HandoffTransition::BeginCapture,
        HandoffTransition::Captured(checkpoint()),
        HandoffTransition::BeginRestore,
        HandoffTransition::Restored,
        HandoffTransition::BeginStart,
        HandoffTransition::Started {
            run_id: "only-one-run".into(),
        },
    ] {
        let previous_revision = r.revision();
        fail.store(true, Ordering::SeqCst);
        assert!(matches!(
            journal.advance(&r, transition).await,
            Err(HandoffError::Storage(_))
        ));
        r = dir.journal().load("operation").await.unwrap();
        assert_eq!(r.revision(), previous_revision + 1);
    }
    assert_eq!(r.run_id(), Some("only-one-run"));
}

#[tokio::test]
async fn malformed_receipts_fail_closed_and_partial_temp_files_are_ignored() {
    let dir = Directory::new();
    let journal = dir.journal();
    let r = journal.create("operation", plan()).await.unwrap();
    std::fs::write(dir.0.join(".operation.interrupted.tmp"), b"{partial").unwrap();
    assert_eq!(journal.load("operation").await.unwrap(), r);
    let path = dir.0.join("6f7065726174696f6e.json");
    let original = serde_json::to_value(&r).unwrap();
    for patch in [
        serde_json::json!({"schema_version":2}),
        serde_json::json!({"id":"other"}),
        serde_json::json!({"phase":"started"}),
        serde_json::json!({"plan":{"source_sandbox_id":"", "target_sandbox_id":"x", "intent":"y"}}),
    ] {
        let mut corrupt = original.clone();
        for (key, value) in patch.as_object().unwrap() {
            corrupt[key] = value.clone();
        }
        std::fs::write(&path, serde_json::to_vec(&corrupt).unwrap()).unwrap();
        assert!(matches!(
            journal.load("operation").await,
            Err(HandoffError::Corrupt)
        ));
        assert!(matches!(
            journal.create("operation", plan()).await,
            Err(HandoffError::Corrupt)
        ));
    }
    std::fs::write(&path, b"{partial").unwrap();
    assert!(matches!(
        journal.load("operation").await,
        Err(HandoffError::Corrupt)
    ));
}

#[tokio::test]
async fn file_permissions_size_limits_and_ids_are_enforced() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Directory::new();
    let journal = dir.journal();
    for id in ["", "../escape", "a/b", "a.b", "a\\b"] {
        assert!(matches!(
            journal.create(id, plan()).await,
            Err(HandoffError::InvalidId)
        ));
    }
    journal.create("operation", plan()).await.unwrap();
    assert_eq!(
        std::fs::metadata(&dir.0).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(dir.0.join("6f7065726174696f6e.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let limited = HandoffJournal::new(
        FileReceiptStore::new(&dir.0)
            .unwrap()
            .with_max_receipt_bytes(10),
    );
    assert!(matches!(
        limited.load("operation").await,
        Err(HandoffError::TooLarge)
    ));
    assert!(matches!(
        limited.create("second", plan()).await,
        Err(HandoffError::TooLarge)
    ));
    assert!(matches!(
        journal.load("second").await,
        Err(HandoffError::NotFound)
    ));
}

#[tokio::test]
async fn debug_output_does_not_expose_checkpoint_or_intent_contents() {
    let dir = Directory::new();
    let journal = dir.journal();
    let r = journal
        .create(
            "private-operation",
            HandoffPlan {
                intent: "secret-intent".into(),
                ..plan()
            },
        )
        .await
        .unwrap();
    let r = journal
        .advance(&r, HandoffTransition::BeginCapture)
        .await
        .unwrap();
    let checkpoint = CheckpointReference {
        reference: "secret-reference".into(),
        digest: Some("secret-digest".into()),
    };
    let r = journal
        .advance(&r, HandoffTransition::Captured(checkpoint.clone()))
        .await
        .unwrap();
    assert!(!format!("{r:?} {:?} {checkpoint:?}", r.plan()).contains("secret-"));
    let error = HandoffError::storage(std::io::Error::other("secret-storage-path"));
    assert!(!format!("{error} {error:?}").contains("secret-storage-path"));
}

#[tokio::test]
async fn mixed_case_ids_have_distinct_files_and_locks() {
    let dir = Directory::new();
    let journal = dir.journal();
    for id in ["operation", "Operation", &"A".repeat(96)] {
        let receipt = journal.create(id, plan()).await.unwrap();
        assert_eq!(journal.load(id).await.unwrap(), receipt);
    }
    assert!(matches!(
        journal.create(&"a".repeat(97), plan()).await,
        Err(HandoffError::InvalidId)
    ));
    assert_eq!(journal.load("operation").await.unwrap().id(), "operation");
    assert_eq!(journal.load("Operation").await.unwrap().id(), "Operation");
}

#[derive(Clone)]
struct PendingAcknowledgement {
    inner: FileReceiptStore,
    committed: Arc<tokio::sync::Notify>,
}
impl ReceiptStore for PendingAcknowledgement {
    async fn load(&self, id: &str) -> HandoffResult<Option<HandoffReceipt>> {
        self.inner.load(id).await
    }
    async fn compare_exchange(
        &self,
        receipt: &HandoffReceipt,
        revision: Option<u64>,
    ) -> HandoffResult<()> {
        self.inner.compare_exchange(receipt, revision).await?;
        self.committed.notify_one();
        std::future::pending().await
    }
}

#[tokio::test]
async fn cancellation_after_commit_requires_reconciliation() {
    let dir = Directory::new();
    let original = dir.journal().create("operation", plan()).await.unwrap();
    let committed = Arc::new(tokio::sync::Notify::new());
    let journal = HandoffJournal::new(PendingAcknowledgement {
        inner: FileReceiptStore::new(&dir.0).unwrap(),
        committed: committed.clone(),
    });
    let task = tokio::spawn(async move {
        journal
            .advance(&original, HandoffTransition::BeginCapture)
            .await
    });
    committed.notified().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let r = dir.journal().load("operation").await.unwrap();
    assert_eq!(r.phase(), HandoffPhase::Capturing);
    assert!(r.phase().needs_reconciliation());
}

#[tokio::test]
async fn file_locks_prevent_other_processes_reading_uncommitted_receipts() {
    let dir = Directory::new();
    dir.journal().create("operation", plan()).await.unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.0.join("6f7065726174696f6e.lock"))
        .unwrap();
    lock.lock().unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "child_process_lock_probe",
            "--nocapture",
        ])
        .env("BOX_CLIENT_TEST_LOCK_DIR", &dir.0)
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    drop(lock);
    assert!(dir.journal().load("operation").await.is_ok());
}

#[tokio::test]
#[ignore = "internal helper spawned by the cross-process lock test"]
async fn child_process_lock_probe() {
    let dir = PathBuf::from(std::env::var("BOX_CLIENT_TEST_LOCK_DIR").unwrap());
    let journal = HandoffJournal::new(FileReceiptStore::new(dir).unwrap());
    assert!(matches!(
        journal.load("operation").await,
        Err(HandoffError::Busy)
    ));
}

#[tokio::test]
async fn application_hooks_run_only_after_claim_and_never_repeat_on_stale_receipts() {
    use std::sync::atomic::AtomicUsize;
    let dir = Directory::new();
    let journal = dir.journal();
    let prepared = journal.create("operation", plan()).await.unwrap();
    let count = AtomicUsize::new(0);
    let calls = &count;
    let r = journal
        .capture_with(&prepared, |pending| async move {
            assert_eq!(pending.phase(), HandoffPhase::Capturing);
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(checkpoint())
        })
        .await
        .unwrap();
    assert!(matches!(
        journal
            .capture_with(&prepared, |_| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(checkpoint())
            })
            .await,
        Err(HandoffError::Conflict)
    ));
    let r = journal
        .restore_with(&r, |pending| async move {
            assert!(pending.checkpoint().is_some());
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .await
        .unwrap();
    let error = journal
        .start_with(&r, |_| async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(HandoffError::storage(std::io::Error::other(
                "launch outcome lost",
            )))
        })
        .await
        .unwrap_err();
    assert!(matches!(error, HandoffError::Storage(_)));
    let pending = journal.load("operation").await.unwrap();
    assert_eq!(pending.phase(), HandoffPhase::Starting);
    assert!(journal
        .start_with(&pending, |_| async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok("duplicate".into())
        })
        .await
        .is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let done = journal
        .advance(
            &pending,
            HandoffTransition::Started {
                run_id: "reconciled-run".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(done.run_id(), Some("reconciled-run"));
}

#[tokio::test]
async fn handoff_logs_report_progress_without_identifiers_or_references() {
    if common::logs::isolated(
        "handoff_logs_report_progress_without_identifiers_or_references",
        "boat_client=trace",
    ) {
        return;
    }
    use tracing::instrument::WithSubscriber;
    let dir = Directory::new();
    let journal = dir.journal();
    let logs = common::logs::Logs::default();
    async {
        let r = journal
            .create(
                "secret-operation",
                HandoffPlan {
                    source_sandbox_id: "secret-source".into(),
                    target_sandbox_id: "secret-target".into(),
                    intent: "secret-intent".into(),
                },
            )
            .await
            .unwrap();
        let pending = journal
            .advance(&r, HandoffTransition::BeginCapture)
            .await
            .unwrap();
        journal
            .advance(
                &pending,
                HandoffTransition::Captured(CheckpointReference {
                    reference: "secret-checkpoint".into(),
                    digest: None,
                }),
            )
            .await
            .unwrap();
        assert!(journal
            .advance(&r, HandoffTransition::BeginCapture)
            .await
            .is_err());
        journal.load("secret-operation").await.unwrap();
    }
    .with_subscriber(logs.subscriber())
    .await;
    let output = logs.text();
    for expected in [
        "receipt created",
        "receipt transition committed",
        "receipt loaded",
        "receipt write not confirmed",
        "phase=Captured",
        "revision=2",
    ] {
        assert!(output.contains(expected), "missing {expected}: {output}");
    }
    assert!(!output.contains("secret-"));
    assert!(!output.contains(dir.0.to_str().unwrap()));
}
