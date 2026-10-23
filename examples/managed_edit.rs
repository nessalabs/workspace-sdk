//! Synthetic local workflow. Never selects or edits a user's actual project.
use std::{fs, sync::Arc};
use workspace_sdk::{
    checkpoint::{GitCheckpointEngine, ReconciliationStatus},
    infrastructure::{FileStateRepository, LocalEditCoordinator},
    EditChange, ManagedEditSession, WorkspacePath,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("project");
    fs::create_dir(&root)?;
    fs::write(root.join("notes.txt"), b"baseline\n")?;
    let engine = Arc::new(
        GitCheckpointEngine::new("git", temp.path().join("checkpoints"))?
            .with_limits(ManagedEditSession::checkpoint_limits())?,
    );
    let repository = Arc::new(FileStateRepository::new(temp.path().join("receipts"))?);
    let coordinator = Arc::new(LocalEditCoordinator::new(&root, repository)?);
    // This fixture has no concurrent processes. A real host must actually pause
    // every unmanaged writer before asserting true at each live-tree boundary.
    let mut edit = ManagedEditSession::begin(coordinator, engine, true)?;
    let revision = edit.stage(
        &edit.revision(),
        EditChange::Replace {
            path: WorkspacePath::new("notes.txt")?,
            bytes: b"agent proposal\n".to_vec(),
            executable: false,
        },
    )?;
    fs::write(root.join("notes.txt"), b"human change between boundaries\n")?;
    let review = edit.review(&revision, true)?;
    assert_eq!(review.status, ReconciliationStatus::Conflicted);
    println!(
        "Review retained {} conflicts without changing the project.",
        review.conflicts.len()
    );
    fs::write(
        review.review_directory.join("notes.txt"),
        b"explicitly resolved human and agent result\n",
    )?;
    let paths = review
        .conflicts
        .iter()
        .map(|conflict| conflict.path.clone())
        .collect::<Vec<_>>();
    edit.accept_resolution(&revision, &review.reference, &paths)?;
    let receipt = edit.apply(&revision, &review.reference, true)?;
    println!(
        "Published the resolved synthetic file; recovery {} retained by the engine.",
        receipt.recovery.0
    );
    Ok(())
}
