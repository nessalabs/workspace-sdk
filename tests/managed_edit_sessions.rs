//! Synthetic local selected-tree editing through real GitCheckpointEngine and shared file leases.
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Barrier},
};
use tempfile::TempDir;
use workspace_sdk::{
    checkpoint::{
        ApplyOptions, ApplyState, GitCheckpointEngine, ICheckpointEngine, ReconciliationStatus,
    },
    infrastructure::{FileStateRepository, LocalEditCoordinator},
    ports::IEditCoordinator,
    *,
};
struct Fixture {
    temp: TempDir,
    root: PathBuf,
    engine: Arc<GitCheckpointEngine>,
    coordinator: Arc<LocalEditCoordinator>,
}
impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("project");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("file"), b"base\n").unwrap();
        fs::write(root.join(".env"), b"synthetic excluded value").unwrap();
        let engine = Arc::new(
            GitCheckpointEngine::new("git", temp.path().join("checkpoints"))
                .unwrap()
                .with_limits(ManagedEditSession::checkpoint_limits())
                .unwrap(),
        );
        let repository = Arc::new(FileStateRepository::new(temp.path().join("receipts")).unwrap());
        let coordinator = Arc::new(LocalEditCoordinator::new(&root, repository).unwrap());
        Self {
            temp,
            root,
            engine,
            coordinator,
        }
    }
    fn session(&self) -> ManagedEditSession {
        ManagedEditSession::begin(self.coordinator.clone(), self.engine.clone(), true).unwrap()
    }
}
fn path(value: &str) -> WorkspacePath {
    WorkspacePath::new(value).unwrap()
}
fn replace(bytes: &[u8]) -> EditChange {
    EditChange::Replace {
        path: path("file"),
        bytes: bytes.to_vec(),
        executable: false,
    }
}
fn create(name: &str, bytes: &[u8]) -> EditChange {
    EditChange::Create {
        path: path(name),
        bytes: bytes.to_vec(),
        executable: false,
    }
}
#[test]
fn candidate_revision_rejects_stale_and_cross_session_inputs_without_touching_live_files() {
    let f = Fixture::new();
    let mut s = f.session();
    let first = s.revision();
    let other = f.session();
    assert!(matches!(
        s.stage(&other.revision(), replace(b"wrong")),
        Err(Error::Conflict(_))
    ));
    let current = s.stage(&first, replace(&[0, 255, 128])).unwrap();
    assert_ne!(current, first);
    assert_eq!(fs::read(f.root.join("file")).unwrap(), b"base\n");
    assert!(matches!(
        s.read(&first, &path("file"), 0, 3),
        Err(Error::Conflict(_))
    ));
    assert_eq!(
        s.read(&current, &path("file"), 0, 3).unwrap().bytes,
        [0, 255, 128]
    );
    assert!(matches!(
        s.stage(&first, replace(b"stale")),
        Err(Error::Conflict(_))
    ));
}
#[test]
fn create_replace_delete_rename_and_binary_apply_use_existing_checkpoint_publication() {
    let f = Fixture::new();
    let mut s = f.session();
    let mut rev = s.revision();
    rev = s.stage(&rev, create("dir/binary", &[0, 255, 128])).unwrap();
    rev = s.stage(&rev, create("scratch", b"temporary")).unwrap();
    rev = s
        .stage(
            &rev,
            EditChange::Delete {
                path: path("scratch"),
            },
        )
        .unwrap();
    rev = s.stage(&rev, replace(b"edited\n")).unwrap();
    rev = s
        .stage(
            &rev,
            EditChange::Rename {
                from: path("file"),
                to: path("renamed"),
            },
        )
        .unwrap();
    assert!(!f.root.join("dir/binary").exists());
    assert_eq!(fs::read(f.root.join("file")).unwrap(), b"base\n");
    let review = s.review(&rev, true).unwrap();
    assert_eq!(review.status, ReconciliationStatus::StagedForReview);
    s.apply(&rev, &review.reference, true).unwrap();
    assert_eq!(fs::read(f.root.join("dir/binary")).unwrap(), [0, 255, 128]);
    assert_eq!(fs::read(f.root.join("renamed")).unwrap(), b"edited\n");
    assert!(!f.root.join("file").exists());
    assert!(!f.root.join("scratch").exists());
    assert_eq!(
        fs::read(f.root.join(".env")).unwrap(),
        b"synthetic excluded value"
    );
}
#[test]
fn human_agent_conflicts_keep_original_sides_and_require_explicit_resolution() {
    let f = Fixture::new();
    let mut s = f.session();
    let rev = s.stage(&s.revision(), replace(b"agent\n")).unwrap();
    fs::write(f.root.join("file"), b"human\n").unwrap();
    let review = s.review(&rev, true).unwrap();
    assert_eq!(review.status, ReconciliationStatus::Conflicted);
    assert_eq!(review.conflicts.len(), 1);
    let conflict = &review.conflicts[0];
    assert_eq!(
        fs::read(&conflict.base.as_ref().unwrap().preserved_path).unwrap(),
        b"base\n"
    );
    assert_eq!(
        fs::read(&conflict.local.as_ref().unwrap().preserved_path).unwrap(),
        b"human\n"
    );
    assert_eq!(
        fs::read(&conflict.remote.as_ref().unwrap().preserved_path).unwrap(),
        b"agent\n"
    );
    assert!(s.apply(&rev, &review.reference, true).is_err());
    assert_eq!(fs::read(f.root.join("file")).unwrap(), b"human\n");
    fs::write(
        review.review_directory.join("file"),
        b"human and agent resolved\n",
    )
    .unwrap();
    assert!(s.accept_resolution(&rev, &review.reference, &[]).is_err());
    let accepted = s
        .accept_resolution(
            &rev,
            &review.reference,
            std::slice::from_ref(&conflict.path),
        )
        .unwrap();
    assert_eq!(accepted.status, ReconciliationStatus::StagedForReview);
    s.apply(&rev, &review.reference, true).unwrap();
    assert_eq!(
        fs::read(f.root.join("file")).unwrap(),
        b"human and agent resolved\n"
    );
}
#[test]
fn changed_candidate_invalidates_review_and_external_change_after_review_is_preserved() {
    let f = Fixture::new();
    let mut s = f.session();
    let rev = s.stage(&s.revision(), replace(b"agent one\n")).unwrap();
    let old = s.review(&rev, true).unwrap();
    let rev = s.stage(&rev, replace(b"agent two\n")).unwrap();
    assert!(matches!(
        s.apply(&rev, &old.reference, true),
        Err(Error::Conflict(_))
    ));
    let current = s.review(&rev, true).unwrap();
    fs::write(f.root.join("file"), b"human after review\n").unwrap();
    assert!(matches!(
        s.apply(&rev, &current.reference, true),
        Err(Error::Conflict(_))
    ));
    assert_eq!(
        fs::read(f.root.join("file")).unwrap(),
        b"human after review\n"
    );
    assert!(f.coordinator.acquire().is_ok());
}
#[test]
fn caller_quiescence_and_shared_nonblocking_lease_are_executable_preconditions() {
    let f = Fixture::new();
    assert!(matches!(
        ManagedEditSession::begin(f.coordinator.clone(), f.engine.clone(), false),
        Err(Error::Conflict(_))
    ));
    let mut s = f.session();
    let rev = s.stage(&s.revision(), replace(b"agent\n")).unwrap();
    assert!(matches!(s.review(&rev, false), Err(Error::Conflict(_))));
    let review = s.review(&rev, true).unwrap();
    assert!(matches!(
        s.apply(&rev, &review.reference, false),
        Err(Error::Conflict(_))
    ));
    let second = LocalEditCoordinator::new(
        &f.root,
        Arc::new(FileStateRepository::new(f.temp.path().join("receipts")).unwrap()),
    )
    .unwrap();
    let held = second.acquire().unwrap();
    assert!(matches!(
        s.apply(&rev, &review.reference, true),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        s.stage(&rev, replace(b"no")),
        Err(Error::Conflict(_))
    ));
    assert_eq!(s.revision(), rev);
    drop(held);
    s.apply(&rev, &review.reference, true).unwrap();
    assert!(second.acquire().is_ok());
}
#[test]
fn nonoverlapping_sessions_reconcile_current_human_and_agent_changes() {
    let f = Fixture::new();
    let mut human = f.session();
    let mut agent = f.session();
    let hr = human
        .stage(&human.revision(), create("human-notes", b"human"))
        .unwrap();
    let ar = agent.stage(&agent.revision(), replace(b"agent\n")).unwrap();
    let review = human.review(&hr, true).unwrap();
    human.apply(&hr, &review.reference, true).unwrap();
    let review = agent.review(&ar, true).unwrap();
    assert_eq!(review.status, ReconciliationStatus::StagedForReview);
    agent.apply(&ar, &review.reference, true).unwrap();
    assert_eq!(fs::read(f.root.join("human-notes")).unwrap(), b"human");
    assert_eq!(fs::read(f.root.join("file")).unwrap(), b"agent\n");
}
#[test]
fn simultaneous_publications_have_one_winner_and_preserve_stale_writer_conflict() {
    let f = Fixture::new();
    let mut human = f.session();
    let mut agent = f.session();
    let hr = human.stage(&human.revision(), replace(b"human\n")).unwrap();
    let ar = agent.stage(&agent.revision(), replace(b"agent\n")).unwrap();
    let hreview = human.review(&hr, true).unwrap();
    let areview = agent.review(&ar, true).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let hbarrier = barrier.clone();
    let (h, a) = std::thread::scope(|scope| {
        let h = scope.spawn(move || {
            hbarrier.wait();
            human.apply(&hr, &hreview.reference, true)
        });
        let a = scope.spawn(move || {
            barrier.wait();
            agent.apply(&ar, &areview.reference, true)
        });
        (h.join().unwrap(), a.join().unwrap())
    });
    assert_eq!(usize::from(h.is_ok()) + usize::from(a.is_ok()), 1);
    assert!(matches!(
        (&h, &a),
        (Ok(_), Err(Error::Conflict(_))) | (Err(Error::Conflict(_)), Ok(_))
    ));
    let bytes = fs::read(f.root.join("file")).unwrap();
    assert!(bytes == b"human\n" || bytes == b"agent\n");
}
#[test]
fn policy_boundaries_and_failed_stage_keep_candidate_and_review_unchanged() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    symlink("file", f.root.join("link")).unwrap();
    let mut s = f.session();
    let rev = s.revision();
    let review = s.review(&rev, true).unwrap();
    for change in [
        create(".env", b"blocked"),
        create("FILE", b"case collision"),
        create("link/child", b"escape"),
        EditChange::Replace {
            path: path("link"),
            bytes: b"no".to_vec(),
            executable: false,
        },
    ] {
        assert!(s.stage(&rev, change).is_err());
        assert_eq!(s.revision(), rev);
    }
    assert!(matches!(
        s.read(&rev, &path("link"), 0, 32),
        Err(Error::Unsupported(_))
    ));
    assert!(s.inspect_apply(&review.reference).is_ok());
    assert_eq!(fs::read(f.root.join("file")).unwrap(), b"base\n");
    assert!(WorkspacePath::new("../outside").is_err());
    assert!(matches!(
        s.stage(
            &rev,
            EditChange::Rename {
                from: path("file"),
                to: path("link")
            }
        ),
        Err(Error::Conflict(_))
    ));
}
#[test]
fn root_replacement_rejects_the_old_session_and_releases_its_lease() {
    let f = Fixture::new();
    let mut s = f.session();
    let rev = s.revision();
    let moved = f.temp.path().join("moved");
    fs::rename(&f.root, &moved).unwrap();
    fs::create_dir(&f.root).unwrap();
    fs::write(f.root.join("file"), b"new directory owner").unwrap();
    assert!(matches!(
        s.stage(&rev, replace(b"wrong root")),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(s.review(&rev, true), Err(Error::Conflict(_))));
    assert_eq!(
        fs::read(f.root.join("file")).unwrap(),
        b"new directory owner"
    );
    assert_eq!(fs::read(moved.join("file")).unwrap(), b"base\n");
}
#[test]
fn completed_retry_preserves_later_edits_and_engine_receipt_survives_restart() {
    let f = Fixture::new();
    let mut s = f.session();
    let rev = s.stage(&s.revision(), replace(b"agent\n")).unwrap();
    let review = s.review(&rev, true).unwrap();
    let receipt = s.apply(&rev, &review.reference, true).unwrap();
    fs::write(f.root.join("file"), b"later human edit\n").unwrap();
    assert_eq!(
        s.apply(&rev, &review.reference, true).unwrap().recovery,
        receipt.recovery
    );
    assert!(matches!(
        s.stage(&rev, replace(b"no replay")),
        Err(Error::Conflict(_))
    ));
    drop(s);
    let reopened = GitCheckpointEngine::new("git", f.temp.path().join("checkpoints")).unwrap();
    assert!(
        matches!(reopened.inspect_apply(&review.reference).unwrap(),ApplyState::Applied{receipt:r}if r.recovery==receipt.recovery)
    );
    let replay = reopened
        .apply(
            &review.reference,
            ApplyOptions {
                quiescent: true,
                expected_fingerprint: review.expected_fingerprint,
            },
        )
        .unwrap();
    assert_eq!(replay.recovery, receipt.recovery);
    assert_eq!(
        fs::read(f.root.join("file")).unwrap(),
        b"later human edit\n"
    );
}
#[test]
fn interrupted_engine_receipt_blocks_new_work_and_uses_existing_recovery() {
    let f = Fixture::new();
    let mut s = f.session();
    let rev = s.stage(&s.revision(), replace(b"agent\n")).unwrap();
    let review = s.review(&rev, true).unwrap();
    let receipt = s.apply(&rev, &review.reference, true).unwrap();
    // Synthetic persisted intent models death after publication before acknowledgement,
    // following the existing checkpoint-engine recovery fixture. No user data is used.
    let plan = f
        .engine
        .state_directory()
        .join("reconciliations")
        .join(&review.reference.0)
        .join("reconciliation.json");
    let journal = f
        .engine
        .state_directory()
        .join("recoveries")
        .join(&receipt.recovery.0)
        .join("journal.json");
    let mut p: serde_json::Value = serde_json::from_slice(&fs::read(&plan).unwrap()).unwrap();
    p["status"] = "staged_for_review".into();
    p["applied"] = serde_json::Value::Null;
    fs::write(&plan, serde_json::to_vec(&p).unwrap()).unwrap();
    let mut j: serde_json::Value = serde_json::from_slice(&fs::read(&journal).unwrap()).unwrap();
    j["status"] = "applying".into();
    fs::write(&journal, serde_json::to_vec(&j).unwrap()).unwrap();
    assert!(matches!(
        s.inspect_apply(&review.reference).unwrap(),
        ApplyState::Interrupted { .. }
    ));
    assert!(matches!(
        s.stage(&rev, replace(b"wrong retry")),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(s.review(&rev, true), Err(Error::Conflict(_))));
    assert!(s.accept_resolution(&rev, &review.reference, &[]).is_err());
    assert_eq!(
        s.apply(&rev, &review.reference, true).unwrap().recovery,
        receipt.recovery
    );
    assert_eq!(fs::read(f.root.join("file")).unwrap(), b"agent\n");
}
#[test]
fn session_resource_and_attempt_limits_are_explicit() {
    let f = Fixture::new();
    let mut s = f.session();
    let mut rev = s.revision();
    assert!(matches!(
        s.stage(&rev, replace(&vec![0; 262145])),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(s.revision(), rev);
    assert!(s.read(&rev, &path("file"), 0, 262145).is_err());
    assert!(s.read(&rev, &path("file"), 999, 1).is_err());
    for index in 0..32 {
        rev = s
            .stage(&rev, replace(format!("candidate{index}\n").as_bytes()))
            .unwrap();
    }
    assert!(matches!(
        s.stage(&rev, replace(b"over budget")),
        Err(Error::Unsupported(_))
    ));
    assert_eq!(fs::read(f.root.join("file")).unwrap(), b"base\n");
    fs::write(f.root.join("file"), b"human at final allowed stage\n").unwrap();
    let review = s.review(&rev, true).unwrap();
    assert_eq!(review.status, ReconciliationStatus::Conflicted);
    fs::write(
        review.review_directory.join("file"),
        b"final resolved candidate\n",
    )
    .unwrap();
    s.accept_resolution(&rev, &review.reference, &["file".to_owned()])
        .unwrap();
    s.apply(&rev, &review.reference, true).unwrap();
    assert_eq!(
        fs::read(f.root.join("file")).unwrap(),
        b"final resolved candidate\n"
    );
    fs::write(f.root.join("large"), vec![0; 8 * 1024 * 1024 + 1]).unwrap();
    assert!(ManagedEditSession::begin(f.coordinator.clone(), f.engine.clone(), true).is_err());
}

#[test]
fn ignored_preexisting_human_destination_is_rejected_before_apply_or_rollback() {
    let f = Fixture::new();
    fs::write(f.root.join(".gitignore"), b"keep\n").unwrap();
    fs::write(f.root.join("keep"), b"human ignored bytes\n").unwrap();
    // Git ignore selection is meaningful in an actual repository.
    assert!(std::process::Command::new("git")
        .args(["init", "--quiet"])
        .arg(&f.root)
        .status()
        .unwrap()
        .success());
    let mut s = f.session();
    assert!(!s
        .checkpoint()
        .manifest
        .entries
        .iter()
        .any(|entry| entry.path == "keep"));
    let rev = s
        .stage(&s.revision(), create("keep", b"agent candidate\n"))
        .unwrap();
    let review = s.review(&rev, true).unwrap();
    assert_eq!(review.status, ReconciliationStatus::StagedForReview);
    assert!(matches!(
        s.apply(&rev, &review.reference, true),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        s.inspect_apply(&review.reference).unwrap(),
        ApplyState::NotStarted
    ));
    assert_eq!(
        fs::read(f.root.join("keep")).unwrap(),
        b"human ignored bytes\n"
    );
    assert_eq!(fs::read(f.root.join("file")).unwrap(), b"base\n");
    assert!(f.coordinator.acquire().is_ok());
}
#[test]
fn engine_limits_reject_oversized_live_capture_and_resolution_before_retaining_snapshots() {
    let f = Fixture::new();
    let wide = Arc::new(GitCheckpointEngine::new("git", f.temp.path().join("wide")).unwrap());
    assert!(matches!(
        ManagedEditSession::begin(f.coordinator.clone(), wide.clone(), true),
        Err(Error::Unsupported(_))
    ));
    assert_eq!(
        fs::read_dir(wide.state_directory().join("checkpoints"))
            .unwrap()
            .count(),
        0
    );
    let mut s = f.session();
    let rev = s.stage(&s.revision(), replace(b"agent\n")).unwrap();
    let snapshots = fs::read_dir(f.engine.state_directory().join("checkpoints"))
        .unwrap()
        .count();
    fs::write(f.root.join("large"), vec![0; 9 * 1024 * 1024]).unwrap();
    assert!(s.review(&rev, true).is_err());
    assert_eq!(
        fs::read_dir(f.engine.state_directory().join("checkpoints"))
            .unwrap()
            .count(),
        snapshots
    );
    assert_eq!(fs::read(f.root.join("file")).unwrap(), b"base\n");
    fs::remove_file(f.root.join("large")).unwrap();
    let review = s.review(&rev, true).unwrap();
    let snapshots = fs::read_dir(f.engine.state_directory().join("checkpoints"))
        .unwrap()
        .count();
    fs::write(
        review.review_directory.join("large"),
        vec![0; 9 * 1024 * 1024],
    )
    .unwrap();
    assert!(s.accept_resolution(&rev, &review.reference, &[]).is_err());
    assert_eq!(
        fs::read_dir(f.engine.state_directory().join("checkpoints"))
            .unwrap()
            .count(),
        snapshots
    );
    assert!(s.apply(&rev, &review.reference, true).is_err());
    assert_eq!(fs::read(f.root.join("file")).unwrap(), b"base\n");
}

#[test]
#[ignore = "manual synthetic local performance qualification"]
fn managed_edit_performance_samples() {
    let f = Fixture::new();
    for index in 0..256 {
        fs::write(f.root.join(format!("input-{index:04}")), vec![0x41; 4096]).unwrap();
    }
    for sample in 0..3 {
        let begin = std::time::Instant::now();
        let mut s = f.session();
        let begin_ms = begin.elapsed().as_millis();
        let stage = std::time::Instant::now();
        let revision = s
            .stage(&s.revision(), replace(&vec![0x9a; 262144]))
            .unwrap();
        let stage_ms = stage.elapsed().as_millis();
        let review = std::time::Instant::now();
        let plan = s.review(&revision, true).unwrap();
        let review_ms = review.elapsed().as_millis();
        let apply = std::time::Instant::now();
        s.apply(&revision, &plan.reference, true).unwrap();
        let apply_ms = apply.elapsed().as_millis();
        println!("managed_edit_performance sample={sample} fixture_entries=257 fixture_bytes_at_least=1048576 staged_bytes=262144 begin_ms={begin_ms} stage_ms={stage_ms} review_ms={review_ms} apply_ms={apply_ms}");
    }
}

#[test]
fn oversized_mutation_paths_reject_before_candidate_allocation_or_snapshot_work() {
    let f = Fixture::new();
    let mut s = f.session();
    let revision = s.revision();
    let snapshots = fs::read_dir(f.engine.state_directory().join("checkpoints"))
        .unwrap()
        .count();
    let huge = WorkspacePath::new("a".repeat(65536)).unwrap();
    for change in [
        EditChange::Create {
            path: huge.clone(),
            bytes: vec![],
            executable: false,
        },
        EditChange::Replace {
            path: huge.clone(),
            bytes: vec![],
            executable: false,
        },
        EditChange::Delete { path: huge.clone() },
        EditChange::Rename {
            from: path("file"),
            to: huge.clone(),
        },
        EditChange::Rename {
            from: huge,
            to: path("file"),
        },
        create(".", b"root"),
    ] {
        assert!(matches!(
            s.stage(&revision, change),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(s.revision(), revision);
    }
    assert_eq!(
        fs::read_dir(f.engine.state_directory().join("checkpoints"))
            .unwrap()
            .count(),
        snapshots
    );
}
