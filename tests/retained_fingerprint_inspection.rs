//! Real retained workspace admission; modeled corruption affects transport replies only.
#[allow(dead_code)]
#[path = "support/recovery_continuation_fixture.rs"]
mod fixture;
mod support;
use fixture::*;
use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    time::Instant,
};
use workspace_sdk::{
    checkpoint::{GitCheckpointEngine, ICheckpointEngine},
    infrastructure::FileStateRepository,
    providers::{ICommandRunner, LocalConfig, LocalProvider, NativeCommand, SystemCommandRunner},
    supervisor::protocol::{SupervisorResponse, SupervisorValue},
    *,
};

fn artifacts(root: &Path) -> usize {
    fs::read_dir(root.join("artifacts/checkpoints"))
        .unwrap()
        .count()
}
fn bytes_on_disk(root: &Path) -> u64 {
    fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            if path.is_dir() {
                bytes_on_disk(&path)
            } else {
                fs::metadata(path).unwrap().len()
            }
        })
        .sum()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retained_admission_creates_no_discarded_checkpoint_while_publication_still_does() {
    run_case("proof-only-retained", |f| async move {
        let engine=Arc::new(GitCheckpointEngine::new("git",f.root.join("artifacts")).unwrap());
        let first=f.initial().await;
        assert_eq!(f.wait(&first).await,RunState::Exited{exit_code:0});
        let root=f.root_of(&first).await;
        fs::write(root.join("large.bin"),vec![0xa7;2*1024*1024]).unwrap();
        let mut head=f.recovery.publish(&f.id,1,true).await.unwrap();
        let workspace=f.sdk.connect(&started(&head).workspace).await.unwrap();
        let sync=SyncService::new(engine.clone(), Arc::new(FileStateRepository::new(f.root.join("authority")).unwrap()));
        let baseline_count=artifacts(&f.root);
        let baseline_bytes=bytes_on_disk(engine.state_directory());
        let capture_started=Instant::now();
        let baseline=sync.capture_workspace(&workspace,true).await.unwrap();
        let capture_ms=capture_started.elapsed().as_secs_f64()*1000.;
        assert_eq!(artifacts(&f.root),baseline_count+1);
        let capture_added_bytes=bytes_on_disk(engine.state_directory())-baseline_bytes;
        let proof_count=artifacts(&f.root);
        let inspect_started=Instant::now();
        assert_eq!(sync.fingerprint_workspace(&workspace,true).await.unwrap(),baseline.manifest.tree_fingerprint);
        let inspect_ms=inspect_started.elapsed().as_secs_f64()*1000.;
        assert_eq!(artifacts(&f.root),proof_count);
        let mut admission_ms=Vec::new();
        for step in 0..2 {
            let before=artifacts(&f.root);
            let start=Instant::now();
            let next=f.reopen().continue_current(&f.id,retained(&format!("proof-step-{step}"),&head)).await.unwrap();
            admission_ms.push(start.elapsed().as_secs_f64()*1000.);
            assert_eq!(artifacts(&f.root),before,"admission must not publish a proof-only artifact");
            assert_eq!(f.wait(&next).await,RunState::Exited{exit_code:0});
            head=f.recovery.publish(&f.id,next.current_attempt.as_ref().unwrap().epoch,true).await.unwrap();
            assert_eq!(artifacts(&f.root),before+1,"acknowledged publication still captures a durable checkpoint");
        }
        assert_eq!(f.run_count().await,3);
        assert_eq!(fs::read_to_string(root.join("counter")).unwrap(),"3");
        println!("{}",serde_json::json!({"scenario":"same_native_2MiB_archive_capture_vs_fingerprint","os":std::env::consts::OS,"arch":std::env::consts::ARCH,"capture_ms":capture_ms,"inspect_ms":inspect_ms,"capture_added_artifacts":1,"capture_added_bytes":capture_added_bytes,"inspection_added_artifacts":0,"retained_admission_ms":admission_ms,"not_statistical_baseline":true,"full_download_unchanged":true}));
    }).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn divergence_and_prelaunch_retry_still_refuse_without_artifacts_or_successors() {
    run_case("proof-only-divergence", |f| async move {
        let head = f.acknowledged().await;
        let root = f.root_of(&head).await;
        let before = artifacts(&f.root);
        fs::write(root.join("counter"), "700").unwrap();
        assert!(matches!(
            f.recovery
                .continue_current(&f.id, retained("next", &head))
                .await,
            Err(Error::Conflict(_))
        ));
        assert_eq!(artifacts(&f.root), before);
        assert_eq!(f.run_count().await, 1);
        fs::write(root.join("counter"), "1").unwrap();
        f.faults.fail_restored.store(true, Ordering::SeqCst);
        f.faults.fail.store(true, Ordering::SeqCst);
        assert!(matches!(
            f.recovery
                .continue_current(&f.id, retained("next", &head))
                .await,
            Err(Error::State(_))
        ));
        assert_eq!(artifacts(&f.root), before);
        assert_eq!(f.run_count().await, 1);
        let pinned = f.session().current_attempt.unwrap();
        fs::write(root.join("counter"), "900").unwrap();
        assert!(matches!(
            f.reopen()
                .continue_current(&f.id, retained("next", &head))
                .await,
            Err(Error::Conflict(_))
        ));
        let refused = f.session();
        let after = refused.current_attempt.as_ref().unwrap();
        assert_eq!(refused.committed, head.committed);
        assert_eq!(after.request_id, pinned.request_id);
        assert_eq!(after.provider, pinned.provider);
        assert_eq!(after.epoch, pinned.epoch);
        assert_eq!(
            after.expected_previous_epoch,
            pinned.expected_previous_epoch
        );
        assert_eq!(after.run_id, pinned.run_id);
        assert_eq!(after.workspace_id, pinned.workspace_id);
        assert_eq!(after.checkpoint, pinned.checkpoint);
        assert_eq!(
            serde_json::to_value(&after.retained).unwrap(),
            serde_json::to_value(&pinned.retained).unwrap()
        );
        assert!(
            matches!(
                after.phase,
                RecoveryPhase::Preparing | RecoveryPhase::Restored { .. }
            ),
            "refused recheck cannot publish a successor launch"
        );
        assert_eq!(fs::read_to_string(root.join("counter")).unwrap(), "900");
        assert_eq!(artifacts(&f.root), before);
        assert_eq!(f.run_count().await, 1);
    })
    .await;
}
struct TransferFault(AtomicU8);
#[async_trait::async_trait]
impl ICommandRunner for TransferFault {
    async fn run(&self, command: NativeCommand) -> Result<ExecResult> {
        let mut result = SystemCommandRunner.run(command).await?;
        if let Ok(SupervisorResponse::Success(SupervisorValue::FileChunk(mut chunk))) =
            serde_json::from_str::<SupervisorResponse>(&result.stdout)
        {
            match self.0.swap(0, Ordering::SeqCst) {
                1 => {
                    let offset = chunk
                        .bytes
                        .windows(7)
                        .position(|b| b == b"counter")
                        .expect("fixture path in first archive chunk");
                    chunk.bytes[offset] = b'C';
                }
                2 => chunk.next_offset += 1,
                _ => return Ok(result),
            }
            result.stdout = serde_json::to_string(&SupervisorResponse::Success(
                SupervisorValue::FileChunk(chunk),
            ))
            .unwrap();
        }
        Ok(result)
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn proof_inspection_preserves_transfer_binding_and_cleans_staging_on_failure() {
    run_case("proof-only-transfer", |f| async move {
        let engine = Arc::new(GitCheckpointEngine::new("git", f.root.join("artifacts")).unwrap());
        let head = f.acknowledged().await;
        let reference = started(&head).workspace;
        let root = f.root_of(&head).await;
        let runner = Arc::new(TransferFault(AtomicU8::new(0)));
        let mut config = LocalConfig::trusted_host(f.root.join("workers/laptop"));
        config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
        let repository = Arc::new(FileStateRepository::new(f.root.join("authority")).unwrap());
        let sdk = WorkspaceSdk::builder(repository.clone())
            .provider(Arc::new(
                LocalProvider::with_runner(config, runner.clone())
                    .unwrap()
                    .with_id(host("laptop")),
            ))
            .unwrap()
            .build()
            .unwrap();
        let workspace = sdk.connect(&reference).await.unwrap();
        let sync = SyncService::new(engine.clone(), repository);
        let before = artifacts(&f.root);
        assert!(matches!(
            sync.fingerprint_workspace(&workspace, false).await,
            Err(Error::Conflict(_))
        ));
        for (mode, expected) in [
            (1, "snapshot transfer checksum mismatch"),
            (2, "target returned an invalid file chunk or offset"),
        ] {
            runner.0.store(mode, Ordering::SeqCst);
            let outcome = sync.fingerprint_workspace(&workspace, true).await;
            assert!(
                matches!(&outcome,Err(Error::State(message)) if message==expected),
                "mode {mode} outcome: {outcome:?}"
            );
            assert_eq!(artifacts(&f.root), before);
            let transfers = root.join(".workspace-sdk/transfers");
            assert!(
                !transfers.exists() || fs::read_dir(transfers).unwrap().next().is_none(),
                "failed proof must remove only its staged transfer"
            );
        }
        let expected = engine
            .export(&head.committed.checkpoint)
            .unwrap()
            .manifest
            .tree_fingerprint;
        assert_eq!(
            sync.fingerprint_workspace(&workspace, true).await.unwrap(),
            expected
        );
        assert_eq!(artifacts(&f.root), before);
        assert_eq!(f.run_count().await, 1);
    })
    .await;
}
