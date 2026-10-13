//! Run after building workspace-supervisor, passing its binary path as the argument.
use std::{env, fs, path::PathBuf, sync::Arc, time::Duration};
use tempfile::tempdir;
use workspace_sdk::{
    checkpoint::{ApplyOptions, GitCheckpointEngine},
    infrastructure::FileStateRepository,
    providers::{LocalConfig, LocalOpenOptions, LocalProvider},
    *,
};

#[tokio::main]
async fn main() -> Result<()> {
    let argument = env::args_os()
        .nth(1)
        .ok_or_else(|| Error::InvalidInput("pass the compiled workspace-supervisor path".into()))?;
    // This same example executable is a tiny Rust workload on the local target.
    if argument == "--worker" {
        fs::write("hello.txt", "Hello from the workspace.\n")?;
        println!("finished");
        return Ok(());
    }
    let supervisor = fs::canonicalize(PathBuf::from(argument))?;
    let demonstration = tempdir()?;
    let project = demonstration.path().join("project");
    fs::create_dir(&project)?;
    fs::write(project.join("hello.txt"), "Hello from the local project.\n")?;
    let state = Arc::new(FileStateRepository::new(
        demonstration.path().join("state"),
    )?);
    let mut local = LocalConfig::trusted_host(demonstration.path().join("targets"));
    local.supervisor = supervisor;
    let sdk = WorkspaceSdk::builder(state.clone())
        .provider(Arc::new(LocalProvider::new(local)?))?
        .build()?;
    let workspace = sdk
        .open(
            &ProviderId::new("local")?,
            OpenRequest::new(WorkspaceId::new("example")?, &LocalOpenOptions::default())?,
        )
        .await?;
    let sync = SyncService::new(
        Arc::new(GitCheckpointEngine::new(
            "git",
            demonstration.path().join("checkpoints"),
        )?),
        state,
    );
    let session = OperationId::new("example-sync")?;
    sync.initialize(session.clone(), &project, &workspace, true)
        .await?;
    let run = workspace
        .start(CommandSpec::new(env::current_exe()?.to_string_lossy()).args(["--worker"]))
        .await?;
    run.wait(Duration::from_secs(10)).await?;
    print!(
        "{}",
        run.output(OutputCursor::default(), 4096).await?.stdout
    );
    let review = sync.sync_back(&session, &workspace, true).await?;
    if review.conflicts.is_empty() {
        // The example owns its temporary project, so no editor can race apply.
        sync.apply_sync(
            &session,
            &review,
            ApplyOptions {
                quiescent: true,
                expected_fingerprint: review.expected_fingerprint.clone(),
            },
        )
        .await?;
        print!("{}", fs::read_to_string(project.join("hello.txt"))?);
    }
    workspace.destroy().await?;
    Ok(())
}
