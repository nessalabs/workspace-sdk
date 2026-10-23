use super::*;
use crate::{infrastructure::MemoryStateRepository, ports::IBackend};
use async_trait::async_trait;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Mutex as StdMutex,
    },
    time::Duration,
};
use tokio::sync::Notify;

#[derive(Clone, Copy, Debug, PartialEq)]
enum BlockAt {
    None,
    Read,
    Write,
    Import,
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Corruption {
    None,
    OversizedChunk,
    EmptyChunk,
    Cursor,
    Short,
    Long,
    Hash,
    OversizedArchive,
    ReadError,
    WriteError,
    ImportError,
}
struct TransferBackend {
    descriptor: BackendDescriptor,
    archive: Vec<u8>,
    corruption: Corruption,
    block_at: BlockAt,
    blocked: AtomicBool,
    entered: Notify,
    release: Notify,
    active: AtomicBool,
    cleanup_count: AtomicUsize,
    export_count: AtomicUsize,
    write_count: AtomicUsize,
    cleanup_during_transfer: AtomicBool,
    imported: AtomicBool,
    started_runs: AtomicUsize,
    staged: StdMutex<BTreeMap<OperationId, Vec<u8>>>,
}
impl TransferBackend {
    fn workspace(self: &Arc<Self>) -> Workspace {
        Workspace {
            backend: self.clone(),
        }
    }
    async fn pause(&self, at: BlockAt) {
        if self.block_at == at && !self.blocked.swap(true, Ordering::SeqCst) {
            self.active.store(true, Ordering::SeqCst);
            self.entered.notify_one();
            self.release.notified().await;
            self.active.store(false, Ordering::SeqCst);
        }
    }
    async fn wait_for_cleanup(&self) {
        tokio::time::timeout(Duration::from_secs(15), async {
            while self.cleanup_count.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("transfer must finish scoped cleanup");
        assert!(!self.cleanup_during_transfer.load(Ordering::SeqCst));
        assert!(self.staged.lock().unwrap().is_empty());
    }
}
fn unsupported<T>() -> Result<T> {
    Err(Error::Unsupported("unused fixture operation".into()))
}
#[async_trait]
impl IBackend for TransferBackend {
    fn descriptor(&self) -> BackendDescriptor {
        self.descriptor.clone()
    }
    async fn ensure_ready(&self) -> Result<()> {
        Ok(())
    }
    async fn refresh(&self) -> Result<WorkspaceState> {
        unsupported()
    }
    async fn exec(&self, _: ExecutionRequest) -> Result<ExecResult> {
        unsupported()
    }
    async fn start(&self, request: LaunchRequest) -> Result<RunRef> {
        self.started_runs.fetch_add(1, Ordering::SeqCst);
        Ok(RunRef {
            workspace: self.descriptor.reference.clone(),
            id: request.id,
        })
    }
    async fn inspect_run(&self, _: &RunRef) -> Result<RunState> {
        Ok(RunState::Exited { exit_code: 0 })
    }
    async fn read_output(&self, _: &RunRef, _: OutputCursor, _: u32) -> Result<OutputPage> {
        unsupported()
    }
    async fn cancel(&self, _: &RunRef) -> Result<RunState> {
        unsupported()
    }
    async fn export_tree(&self, transfer: &TreeTransfer) -> Result<TreeExportReceipt> {
        self.export_count.fetch_add(1, Ordering::SeqCst);
        self.staged
            .lock()
            .unwrap()
            .insert(transfer.id.clone(), self.archive.clone());
        Ok(TreeExportReceipt {
            bytes: if self.corruption == Corruption::OversizedArchive {
                MAX_ARCHIVE as u64 + 1
            } else {
                self.archive.len() as u64
            },
            sha256: if self.corruption == Corruption::Hash {
                "0".repeat(64)
            } else {
                format!("{:x}", Sha256::digest(&self.archive))
            },
            files: 1,
        })
    }
    async fn read_file(&self, request: ReadFile) -> Result<FileChunk> {
        self.pause(BlockAt::Read).await;
        if self.corruption == Corruption::ReadError {
            return Err(Error::State("injected read failure".into()));
        }
        let offset = request.offset as usize;
        let end = (offset + request.limit as usize).min(self.archive.len());
        let mut bytes = self.archive[offset..end].to_vec();
        match self.corruption {
            Corruption::OversizedChunk => bytes = self.archive.clone(),
            Corruption::EmptyChunk => bytes.clear(),
            Corruption::Short => {
                bytes.pop();
            }
            Corruption::Long => bytes.push(0),
            _ => {}
        }
        Ok(FileChunk {
            next_offset: request.offset
                + bytes.len() as u64
                + u64::from(self.corruption == Corruption::Cursor),
            eof: self.corruption != Corruption::EmptyChunk
                && (self.corruption == Corruption::OversizedChunk || end == self.archive.len()),
            bytes,
        })
    }
    async fn write_file(&self, request: WriteFile) -> Result<()> {
        self.write_count.fetch_add(1, Ordering::SeqCst);
        let id = OperationId::new(request.path.as_str().split('/').nth(2).unwrap()).unwrap();
        {
            let mut staged = self.staged.lock().unwrap();
            let bytes = staged.entry(id).or_default();
            if request.truncate {
                bytes.clear();
            }
            assert_eq!(bytes.len() as u64, request.offset);
            bytes.extend_from_slice(&request.bytes);
        }
        self.pause(BlockAt::Write).await;
        if self.corruption == Corruption::WriteError {
            return Err(Error::State("injected write failure".into()));
        }
        Ok(())
    }
    async fn import_tree(&self, transfer: &TreeTransfer) -> Result<TreeImportReceipt> {
        self.pause(BlockAt::Import).await;
        if self.corruption == Corruption::ImportError {
            return Err(Error::State("injected import failure".into()));
        }
        assert_eq!(
            self.staged.lock().unwrap().get(&transfer.id).unwrap(),
            &self.archive
        );
        self.imported.store(true, Ordering::SeqCst);
        Ok(TreeImportReceipt { files: 1 })
    }
    async fn remove_transfer(&self, transfer: &TreeTransfer) -> Result<()> {
        self.cleanup_during_transfer
            .store(self.active.load(Ordering::SeqCst), Ordering::SeqCst);
        self.staged.lock().unwrap().remove(&transfer.id);
        self.cleanup_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
struct Fixture {
    _temporary: tempfile::TempDir,
    root: std::path::PathBuf,
    engine: Arc<GitCheckpointEngine>,
    repository: Arc<MemoryStateRepository>,
    backend: Arc<TransferBackend>,
}
impl Fixture {
    fn new(block_at: BlockAt, corruption: Corruption) -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("project");
        fs::create_dir(&root).unwrap();
        fs::write(
            root.join("file.txt"),
            if corruption == Corruption::OversizedChunk {
                vec![b'x'; CHUNK + 1]
            } else {
                b"portable content\n".to_vec()
            },
        )
        .unwrap();
        let engine = Arc::new(
            GitCheckpointEngine::new("git", temporary.path().join("checkpoints")).unwrap(),
        );
        let archive = engine.export_tree_archive(&root).unwrap();
        let backend = Arc::new(TransferBackend {
            descriptor: BackendDescriptor {
                reference: WorkspaceRef {
                    version: 1,
                    id: WorkspaceId::new("fixture").unwrap(),
                    provider: ProviderId::new("fixture").unwrap(),
                    locator: WorkspaceLocator::from_locator(
                        &serde_json::json!({"target":"fixture"}),
                    )
                    .unwrap(),
                },
                root: "/fixture/project".into(),
                control_root: "/fixture/control".into(),
                isolation: Isolation::TrustedHost,
                ownership: Ownership::SdkOwned,
                capabilities: Capabilities {
                    managed_runs: true,
                    ..Capabilities::default()
                },
            },
            archive,
            corruption,
            block_at,
            blocked: AtomicBool::new(false),
            entered: Notify::new(),
            release: Notify::new(),
            active: AtomicBool::new(false),
            cleanup_count: AtomicUsize::new(0),
            export_count: AtomicUsize::new(0),
            write_count: AtomicUsize::new(0),
            cleanup_during_transfer: AtomicBool::new(false),
            imported: AtomicBool::new(false),
            started_runs: AtomicUsize::new(0),
            staged: StdMutex::new(BTreeMap::new()),
        });
        Self {
            _temporary: temporary,
            root,
            engine,
            repository: Arc::new(MemoryStateRepository::default()),
            backend,
        }
    }
    fn service(&self) -> SyncService {
        SyncService::new(self.engine.clone(), self.repository.clone())
    }
}

#[tokio::test]
async fn canceled_download_finishes_before_scoped_cleanup() {
    let fixture = Fixture::new(BlockAt::Read, Corruption::None);
    let service = fixture.service();
    let workspace = fixture.backend.workspace();
    let observer = tokio::spawn(async move { service.capture_workspace(&workspace, true).await });
    tokio::time::timeout(Duration::from_secs(15), fixture.backend.entered.notified())
        .await
        .unwrap();
    observer.abort();
    assert!(observer.await.unwrap_err().is_cancelled());
    assert_eq!(fixture.backend.cleanup_count.load(Ordering::SeqCst), 0);
    fixture.backend.release.notify_one();
    fixture.backend.wait_for_cleanup().await;
}

#[tokio::test]
async fn canceled_initialization_holds_lease_through_upload_and_import() {
    for block_at in [BlockAt::Write, BlockAt::Import] {
        let fixture = Fixture::new(block_at, Corruption::None);
        let service = fixture.service();
        let workspace = fixture.backend.workspace();
        let root = fixture.root.clone();
        let id = OperationId::new("cancel-initialize").unwrap();
        let operation_id = id.clone();
        let observer = tokio::spawn(async move {
            service
                .initialize(operation_id, root, &workspace, true)
                .await
        });
        tokio::time::timeout(Duration::from_secs(15), fixture.backend.entered.notified())
            .await
            .unwrap();
        observer.abort();
        assert!(observer.await.unwrap_err().is_cancelled());
        assert!(matches!(
            fixture.repository.acquire(&id),
            Err(Error::Conflict(_))
        ));
        assert_eq!(fixture.backend.cleanup_count.load(Ordering::SeqCst), 0);
        fixture.backend.release.notify_one();
        fixture.backend.wait_for_cleanup().await;
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if fixture.service().session(&id).unwrap().initialized {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(fixture.backend.imported.load(Ordering::SeqCst));
        tokio::time::timeout(Duration::from_secs(15), async {
            while fixture.repository.acquire(&id).is_err() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn malformed_downloads_fail_and_cleanup_without_accepting_checkpoint() {
    for corruption in [
        Corruption::OversizedChunk,
        Corruption::EmptyChunk,
        Corruption::Cursor,
        Corruption::Short,
        Corruption::Long,
        Corruption::Hash,
        Corruption::OversizedArchive,
        Corruption::ReadError,
    ] {
        let fixture = Fixture::new(BlockAt::None, corruption);
        let result = fixture
            .service()
            .capture_workspace(&fixture.backend.workspace(), true)
            .await;
        assert!(result.is_err(), "{corruption:?}");
        fixture.backend.wait_for_cleanup().await;
        assert_eq!(
            fs::read_dir(fixture.engine.state_directory().join("checkpoints"))
                .unwrap()
                .count(),
            0
        );
    }
}

#[tokio::test]
async fn valid_download_keeps_exact_file_bytes_and_cleans_staging() {
    let fixture = Fixture::new(BlockAt::None, Corruption::None);
    let checkpoint = fixture
        .service()
        .capture_workspace(&fixture.backend.workspace(), true)
        .await
        .unwrap();
    let destination = fixture._temporary.path().join("restored");
    fixture
        .engine
        .restore(checkpoint.reference(), &destination)
        .unwrap();
    assert_eq!(
        fs::read(destination.join("file.txt")).unwrap(),
        b"portable content\n"
    );
    fixture.backend.wait_for_cleanup().await;
}

#[tokio::test]
async fn canceled_restore_finishes_upload_and_import_before_cleanup() {
    for block_at in [BlockAt::Write, BlockAt::Import] {
        let fixture = Fixture::new(block_at, Corruption::None);
        let checkpoint = fixture
            .engine
            .capture(&fixture.root, CaptureOptions::default())
            .unwrap();
        let reference = checkpoint.reference().clone();
        let service = fixture.service();
        let workspace = fixture.backend.workspace();
        let observer = tokio::spawn(async move {
            service
                .restore_checkpoint(&reference, &workspace, true)
                .await
        });
        tokio::time::timeout(Duration::from_secs(15), fixture.backend.entered.notified())
            .await
            .unwrap();
        observer.abort();
        assert!(observer.await.unwrap_err().is_cancelled());
        assert_eq!(fixture.backend.cleanup_count.load(Ordering::SeqCst), 0);
        fixture.backend.release.notify_one();
        fixture.backend.wait_for_cleanup().await;
        assert!(fixture.backend.imported.load(Ordering::SeqCst));
    }
}

#[tokio::test]
async fn canceled_sync_back_holds_lease_until_review_receipt_is_saved() {
    let fixture = Fixture::new(BlockAt::Read, Corruption::None);
    let id = OperationId::new("cancel-sync-back").unwrap();
    fixture
        .service()
        .initialize(
            id.clone(),
            &fixture.root,
            &fixture.backend.workspace(),
            true,
        )
        .await
        .unwrap();
    let service = fixture.service();
    let workspace = fixture.backend.workspace();
    let operation_id = id.clone();
    let observer =
        tokio::spawn(async move { service.sync_back(&operation_id, &workspace, true).await });
    tokio::time::timeout(Duration::from_secs(15), fixture.backend.entered.notified())
        .await
        .unwrap();
    observer.abort();
    assert!(observer.await.unwrap_err().is_cancelled());
    assert!(matches!(
        fixture.repository.acquire(&id),
        Err(Error::Conflict(_))
    ));
    assert_eq!(fixture.backend.cleanup_count.load(Ordering::SeqCst), 1);
    fixture.backend.release.notify_one();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Ok(_lease) = fixture.repository.acquire(&id) {
                assert!(fixture
                    .service()
                    .session(&id)
                    .unwrap()
                    .latest_reconciliation
                    .is_some());
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(fixture.backend.cleanup_count.load(Ordering::SeqCst), 2);
    fixture.backend.wait_for_cleanup().await;
}

#[tokio::test]
async fn canceled_handoff_retains_lease_and_records_only_one_continuation() {
    let source = Fixture::new(BlockAt::None, Corruption::None);
    let mut destination = Fixture::new(BlockAt::Import, Corruption::None);
    Arc::get_mut(&mut destination.backend)
        .unwrap()
        .descriptor
        .reference
        .id = WorkspaceId::new("destination").unwrap();
    let service = source.service();
    let source_workspace = source.backend.workspace();
    let source_run = source_workspace.run(RunId::new("stopped-source").unwrap());
    let destination_workspace = destination.backend.workspace();
    let id = OperationId::new("cancel-handoff").unwrap();
    let operation_id = id.clone();
    let command = CommandSpec::new("continuation");
    let continuation = command.clone();
    let observer = tokio::spawn(async move {
        service
            .handoff(
                operation_id,
                &source_workspace,
                &source_run,
                &destination_workspace,
                continuation.into(),
                true,
            )
            .await
    });
    tokio::time::timeout(
        Duration::from_secs(15),
        destination.backend.entered.notified(),
    )
    .await
    .unwrap();
    observer.abort();
    assert!(observer.await.unwrap_err().is_cancelled());
    assert!(matches!(
        source.repository.acquire(&id),
        Err(Error::Conflict(_))
    ));
    assert_eq!(destination.backend.cleanup_count.load(Ordering::SeqCst), 0);
    destination.backend.release.notify_one();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Ok(_lease) = source.repository.acquire(&id) {
                let receipt = source
                    .repository
                    .load(&id)
                    .unwrap()
                    .unwrap()
                    .into_handoff()
                    .unwrap();
                assert!(matches!(receipt.phase, HandoffPhase::Started));
                assert!(receipt.destination_run.is_some());
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let workspace = source.backend.workspace();
    source
        .service()
        .handoff(
            id,
            &workspace,
            &workspace.run(RunId::new("stopped-source").unwrap()),
            &destination.backend.workspace(),
            command.into(),
            true,
        )
        .await
        .unwrap();
    assert_eq!(destination.backend.started_runs.load(Ordering::SeqCst), 1);
    source.backend.wait_for_cleanup().await;
    destination.backend.wait_for_cleanup().await;
}

#[tokio::test]
async fn canceling_a_queued_workflow_does_not_start_it_later() {
    let fixture = Fixture::new(BlockAt::Write, Corruption::None);
    let service = fixture.service();
    let first_service = service.clone();
    let workspace = fixture.backend.workspace();
    let root = fixture.root.clone();
    let first = tokio::spawn(async move {
        first_service
            .initialize(OperationId::new("first").unwrap(), root, &workspace, true)
            .await
    });
    tokio::time::timeout(Duration::from_secs(15), fixture.backend.entered.notified())
        .await
        .unwrap();
    let queued_id = OperationId::new("queued").unwrap();
    assert!(tokio::time::timeout(
        Duration::from_millis(20),
        service.initialize(
            queued_id.clone(),
            &fixture.root,
            &fixture.backend.workspace(),
            true
        )
    )
    .await
    .is_err());
    fixture.backend.release.notify_one();
    first.await.unwrap().unwrap();
    assert!(fixture.repository.load(&queued_id).unwrap().is_none());
    assert_eq!(fixture.backend.cleanup_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn canceled_queued_direct_transfers_do_not_accept_background_work() {
    let fixture = Fixture::new(BlockAt::Read, Corruption::None);
    let checkpoint = fixture
        .engine
        .capture(&fixture.root, CaptureOptions::default())
        .unwrap();
    let service = fixture.service();
    let first_service = service.clone();
    let workspace = fixture.backend.workspace();
    let first =
        tokio::spawn(async move { first_service.capture_workspace(&workspace, true).await });
    tokio::time::timeout(Duration::from_secs(15), fixture.backend.entered.notified())
        .await
        .unwrap();
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    assert!(tokio::time::timeout(
        Duration::from_millis(20),
        service.capture_workspace(&fixture.backend.workspace(), true)
    )
    .await
    .is_err());
    assert!(tokio::time::timeout(
        Duration::from_millis(20),
        service.restore_checkpoint(checkpoint.reference(), &fixture.backend.workspace(), true)
    )
    .await
    .is_err());
    assert_eq!(fixture.backend.export_count.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.backend.write_count.load(Ordering::SeqCst), 0);
    fixture.backend.release.notify_one();
    fixture.backend.wait_for_cleanup().await;
    // Crossing the admission guard proves the accepted worker and all queued
    // observers have finished before checking that no later effects appeared.
    let _guard = service.operations.lock().await;
    assert_eq!(fixture.backend.export_count.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.backend.write_count.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.backend.cleanup_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn upload_and_import_failures_preserve_original_error_and_cleanup_staging() {
    for (corruption, expected) in [
        (Corruption::WriteError, "injected write failure"),
        (Corruption::ImportError, "injected import failure"),
    ] {
        let fixture = Fixture::new(BlockAt::None, corruption);
        let checkpoint = fixture
            .engine
            .capture(&fixture.root, CaptureOptions::default())
            .unwrap();
        let error = fixture
            .service()
            .restore_checkpoint(checkpoint.reference(), &fixture.backend.workspace(), true)
            .await
            .unwrap_err();
        assert!(error.to_string().contains(expected));
        assert!(!fixture.backend.imported.load(Ordering::SeqCst));
        fixture.backend.wait_for_cleanup().await;
    }
}
