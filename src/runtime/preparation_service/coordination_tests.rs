use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

struct FaultStore {
    inner: Arc<MemoryPreparationStore>,
    fail_process_save: AtomicBool,
    fail_after_commit: bool,
    alter_scope: bool,
}

#[async_trait]
impl IPreparationStore for FaultStore {
    async fn try_acquire(&self, id: &str) -> Result<Box<dyn IPreparationLease>> {
        self.inner.try_acquire(id).await
    }
    async fn load(&self, id: &str) -> Result<Option<PreparationReceipt>> {
        let mut receipt = self.inner.load(id).await?;
        if self.alter_scope {
            if let Some(value) = receipt.as_mut() {
                value.host_scope = "foreign-host".into();
            }
        }
        Ok(receipt)
    }
    async fn save(&self, receipt: &PreparationReceipt) -> Result<()> {
        if receipt.authentication_process.is_some()
            && self.fail_process_save.swap(false, Ordering::SeqCst)
        {
            if self.fail_after_commit {
                self.inner.save(receipt).await?;
            }
            return Err(Error::State(
                "injected receipt acknowledgement failure".into(),
            ));
        }
        self.inner.save(receipt).await
    }
}

#[tokio::test]
async fn failed_launch_receipt_acknowledgement_reconnects_to_one_native_process() {
    for fail_after_commit in [false, true] {
        let host = Arc::new(FakeHost::default());
        let store = Arc::new(FaultStore {
            inner: Arc::new(MemoryPreparationStore::default()),
            fail_process_save: AtomicBool::new(true),
            fail_after_commit,
            alter_scope: false,
        });
        let first = PreparationService::new(host.clone(), Arc::new(NoCredentials), store.clone());
        let request = request(codex("0.20.0"), Authentication::Subscription);
        let PreparationOutcome::NeedsAuthentication { action, .. } =
            first.prepare(&request).await.unwrap()
        else {
            panic!("expected native authentication")
        };
        assert!(first.start_authentication(&request, &action).await.is_err());
        let resumed = PreparationService::new(host.clone(), Arc::new(NoCredentials), store.clone());
        let session = resumed
            .start_authentication(&request, &action)
            .await
            .unwrap();
        assert!(session.process.is_some());
        assert_eq!(host.0.lock().unwrap().login_calls, 1);
        assert_eq!(
            store
                .load(&request.id)
                .await
                .unwrap()
                .unwrap()
                .authentication_process,
            session.process
        );
    }
}

#[tokio::test]
async fn service_instances_share_operation_ownership_without_blocking_other_preparations() {
    let (first, host, _, store) = service();
    let request = request(codex("0.20.0"), Authentication::Subscription);
    let PreparationOutcome::NeedsAuthentication { action, .. } =
        first.prepare(&request).await.unwrap()
    else {
        panic!("expected login action")
    };
    let resumed = PreparationService::new(host.clone(), Arc::new(NoCredentials), store.clone());
    let lease = store.try_acquire(&request.id).await.unwrap();
    let before = host.0.lock().unwrap().commands.len();
    assert!(matches!(
        resumed.start_authentication(&request, &action).await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(host.0.lock().unwrap().commands.len(), before);
    let mut independent = request.clone();
    independent.id = "independent-preparation".into();
    assert!(matches!(
        resumed.prepare(&independent).await.unwrap(),
        PreparationOutcome::NeedsAuthentication { .. }
    ));
    drop(lease);
    let left = first.start_authentication(&request, &action).await.unwrap();
    let right = resumed
        .start_authentication(&request, &action)
        .await
        .unwrap();
    assert_eq!(left.process, right.process);
    assert_eq!(host.0.lock().unwrap().login_calls, 1);
}

#[tokio::test]
async fn file_leases_exclude_separate_store_instances_and_release_on_drop() {
    let directory = tempfile::tempdir().unwrap();
    let first = FilePreparationStore::new(directory.path()).unwrap();
    let second = FilePreparationStore::new(directory.path()).unwrap();
    let lease = first.try_acquire("same").await.unwrap();
    assert!(matches!(
        second.try_acquire("same").await,
        Err(Error::Conflict(_))
    ));
    let independent = second.try_acquire("another").await.unwrap();
    drop(lease);
    let resumed = second.try_acquire("same").await.unwrap();
    drop((independent, resumed));
    assert!(first.try_acquire("same").await.is_ok());
}

#[tokio::test]
async fn same_request_cannot_rebind_a_receipt_to_another_host_scope() {
    let (service, host, _, store) = service();
    let request = request(codex("0.20.0"), Authentication::Subscription);
    service.prepare(&request).await.unwrap();
    let routed = Arc::new(FaultStore {
        inner: store,
        fail_process_save: AtomicBool::new(false),
        fail_after_commit: false,
        alter_scope: true,
    });
    let resumed = PreparationService::new(host.clone(), Arc::new(NoCredentials), routed);
    let calls = host.0.lock().unwrap().commands.len();
    assert!(matches!(
        resumed.prepare(&request).await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(host.0.lock().unwrap().commands.len(), calls);
}
