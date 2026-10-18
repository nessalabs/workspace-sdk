//! Conditional recovery binds the acknowledged head under the SDK effect lease.
// Shared native fixture also serves the broader continuation suite.
#[allow(dead_code)]
#[path = "support/recovery_continuation_fixture.rs"]
mod fixture;
mod support;
use fixture::*;
use workspace_sdk::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn conditional_recovery_refuses_superseded_head_and_retries_its_original_checkpoint() {
    run_case("conditional-recovery-head", |fixture| async move {
        let first = fixture.acknowledged().await;
        let old_head = first.committed.clone();
        // A second actual publication changes the head without changing the
        // source epoch. Epoch-only admission cannot distinguish these heads.
        let advanced = fixture
            .recovery
            .publish(&fixture.id, 1, true)
            .await
            .unwrap();
        assert_ne!(
            snapshot(&first)["committed"],
            snapshot(&advanced)["committed"]
        );
        let before = snapshot(&advanced);
        let request = || fresh("conditional-devbox", 1, "devbox");
        assert!(matches!(
            fixture
                .recovery
                .recover_at_head(&fixture.id, &old_head, request())
                .await,
            Err(Error::Conflict(_))
        ));
        assert_eq!(snapshot(&fixture.session()), before);
        assert_eq!(fixture.sdk.list().unwrap().len(), 1);
        assert_eq!(fixture.run_count().await, 1);
        let accepted = fixture
            .recovery
            .recover_at_head(&fixture.id, &advanced.committed, request())
            .await
            .unwrap();
        assert_eq!(
            fixture.wait(&accepted).await,
            RunState::Exited { exit_code: 0 }
        );
        let published = fixture
            .recovery
            .publish(&fixture.id, 2, true)
            .await
            .unwrap();
        let retry = fixture
            .reopen()
            .recover_at_head(&fixture.id, &advanced.committed, request())
            .await
            .unwrap();
        same(&started(&retry), &started(&accepted));
        assert_eq!(snapshot(&retry), snapshot(&published));
        assert!(matches!(
            fixture
                .reopen()
                .recover_at_head(&fixture.id, &published.committed, request())
                .await,
            Err(Error::Conflict(_))
        ));
        assert_eq!(snapshot(&fixture.session()), snapshot(&published));
        assert_eq!(fixture.run_count().await, 2);
        assert_eq!(fixture.sdk.list().unwrap().len(), 2);
    })
    .await;
}

struct BeforeLease {
    inner: std::sync::Arc<workspace_sdk::infrastructure::FileStateRepository>,
    entered: tokio::sync::Notify,
    release: std::sync::Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}
impl workspace_sdk::ports::IRecoveryRepository for BeforeLease {
    fn acquire_recovery(
        &self,
        id: &OperationId,
    ) -> Result<Box<dyn workspace_sdk::ports::IWorkflowLease>> {
        if let Some(release) = self.release.lock().unwrap().take() {
            self.entered.notify_one();
            release
                .recv_timeout(std::time::Duration::from_secs(5))
                .map_err(|_| {
                    Error::State("prelease test gate released without acknowledgement".into())
                })?;
        }
        self.inner.acquire_recovery(id)
    }
    fn load_recovery(&self, id: &OperationId) -> Result<Option<RecoverySession>> {
        self.inner.load_recovery(id)
    }
    fn compare_exchange_recovery(
        &self,
        id: &OperationId,
        revision: Option<u64>,
        replacement: &RecoverySession,
    ) -> Result<()> {
        self.inner
            .compare_exchange_recovery(id, revision, replacement)
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn conditional_head_is_checked_after_effect_lease_acquisition() {
    run_case("conditional-head-lease-order", |fixture| async move {
        use std::sync::{mpsc, Arc, Mutex};
        use workspace_sdk::{checkpoint::GitCheckpointEngine, infrastructure::FileStateRepository};
        let first = fixture.acknowledged().await;
        let repository =
            Arc::new(FileStateRepository::new(fixture.root.join("authority")).unwrap());
        let (release, receiver) = mpsc::channel();
        let gated = Arc::new(BeforeLease {
            inner: repository.clone(),
            entered: tokio::sync::Notify::new(),
            release: Mutex::new(Some(receiver)),
        });
        let engine =
            Arc::new(GitCheckpointEngine::new("git", fixture.root.join("artifacts")).unwrap());
        let recovery = RecoveryService::new(
            fixture.sdk.clone(),
            Arc::new(SyncService::new(engine, repository)),
            gated.clone(),
        );
        let expected = first.committed.clone();
        let id = fixture.id.clone();
        let waiter = tokio::spawn(async move {
            recovery
                .recover_at_head(&id, &expected, fresh("lease-ordered-target", 1, "devbox"))
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), gated.entered.notified())
            .await
            .unwrap();
        let advanced = fixture
            .recovery
            .publish(&fixture.id, 1, true)
            .await
            .unwrap();
        release.send(()).unwrap();
        assert!(matches!(waiter.await.unwrap(), Err(Error::Conflict(_))));
        assert_eq!(snapshot(&fixture.session()), snapshot(&advanced));
        assert_eq!(fixture.run_count().await, 1);
        assert_eq!(fixture.sdk.list().unwrap().len(), 1);
    })
    .await;
}
