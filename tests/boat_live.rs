//! Explicitly opted-in, billable native Boat conformance test. Never runs by default.
#![cfg(all(feature = "boat", unix))]
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::mem::zeroed;
use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tempfile::TempDir;
use workspace_sdk::{
    boat_client::{Api, ClientConfig},
    checkpoint::{ApplyOptions, GitCheckpointEngine},
    infrastructure::FileStateRepository,
    providers::{
        SandboxOpenOptions, SandboxProvider, SupervisorArtifact, SupervisorDeployment,
        TargetPlatform,
    },
    CommandSpec, DeleteOperation, DeleteState, Error, OpenRequest, OperationId, OutputCursor,
    ProviderId, ReadFile, Result, RunState, SyncService, Workspace, WorkspaceId, WorkspacePath,
    WorkspaceRef, WorkspaceSdk, WriteFile,
};

#[derive(Serialize)]
struct Check {
    name: &'static str,
}
#[derive(Serialize)]
struct Fixture {
    requested_id: WorkspaceId,
    reference: Option<WorkspaceRef>,
}
#[derive(Serialize)]
struct Cleanup {
    workspace: WorkspaceRef,
    stop_requested: bool,
    deletion: Option<DeleteOperation>,
    error: Option<String>,
}
#[derive(Serialize)]
struct Evidence {
    schema: u32,
    implementation: &'static str,
    supervisor_sha256: String,
    workload_sha256: String,
    checks: Vec<Check>,
    fixtures: Vec<Fixture>,
    cleanup: Vec<Cleanup>,
    functional_passed: bool,
    cleanup_confirmed: bool,
    error: Option<String>,
}
struct Report {
    path: PathBuf,
    evidence: Evidence,
}
impl Report {
    fn save(&self) -> Result<()> {
        fs::write(&self.path, serde_json::to_vec_pretty(&self.evidence)?)?;
        Ok(())
    }
    fn passed(&mut self, name: &'static str) -> Result<()> {
        self.evidence.checks.push(Check { name });
        self.save()?;
        eprintln!("passed: {name}");
        Ok(())
    }
}
fn required(name: &str) -> Result<String> {
    env::var(name).map_err(|_| Error::InvalidInput(format!("explicit live test requires {name}")))
}
fn api_key() -> Result<String> {
    if let Ok(key) = env::var("BOAT_API_KEY") {
        return Ok(key);
    }
    // Tests can accept a key over a PTY without writing it to a file or command argument.
    let mut settings = unsafe { zeroed::<libc::termios>() };
    if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut settings) } != 0 {
        return Err(Error::CredentialRequired("BOAT_API_KEY".into()));
    }
    let original = settings;
    settings.c_lflag &= !libc::ECHO;
    if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &settings) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    eprint!("Boat API key: ");
    io::stderr().flush()?;
    let mut key = String::new();
    let read = io::stdin().read_line(&mut key);
    unsafe {
        libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &original);
    }
    eprintln!();
    read?;
    Ok(key.trim().to_owned())
}

#[tokio::test]
#[ignore = "creates two billable Boates; requires explicitly supplied native Linux artifacts and target paths"]
async fn native_rust_box_workflow() -> Result<()> {
    let supervisor = PathBuf::from(required("BOAT_SUPERVISOR_ARTIFACT")?);
    let workload = PathBuf::from(required("BOAT_WORKLOAD_ARTIFACT")?);
    let target_root = required("BOAT_WORKSPACE_ROOT")?;
    let executable = required("BOAT_SUPERVISOR_EXECUTABLE")?;
    let path = PathBuf::from(required("BOAT_SDK_REPORT")?);
    if path.exists() {
        return Err(Error::Conflict(
            "choose a new evidence path; existing reports are immutable".into(),
        ));
    }
    let api = Api::new(
        ClientConfig::new(api_key()?)
            .map_err(|_| Error::CredentialRequired("Boat API key".into()))?,
    )
    .map_err(|_| Error::State("could not construct the Boat client".into()))?;
    let temporary = TempDir::new()?;
    let state = Arc::new(FileStateRepository::new(temporary.path().join("state"))?);
    let sdk = WorkspaceSdk::builder(state.clone())
        .provider(Arc::new(SandboxProvider::new(api).with_supervisor(
            SupervisorDeployment::Upload {
                artifact: SupervisorArtifact {
                    path: supervisor.clone(),
                    platform: TargetPlatform::LinuxX86_64,
                },
                executable,
            },
        )))?
        .build()?;
    let sync = SyncService::new(
        Arc::new(GitCheckpointEngine::new(
            "git",
            temporary.path().join("checkpoints"),
        )?),
        state,
    );
    let mut report = Report {
        path,
        evidence: Evidence {
            schema: 1,
            implementation: "native Rust supervisor and Rust workload; no model calls",
            supervisor_sha256: format!("{:x}", Sha256::digest(fs::read(supervisor)?)),
            workload_sha256: format!("{:x}", Sha256::digest(fs::read(&workload)?)),
            checks: vec![],
            fixtures: vec![],
            cleanup: vec![],
            functional_passed: false,
            cleanup_confirmed: false,
            error: None,
        },
    };
    report.save()?;
    let mut fixtures = Vec::new();
    let outcome = exercise(
        &sdk,
        &sync,
        temporary.path(),
        &workload,
        &target_root,
        &mut report,
        &mut fixtures,
    )
    .await;
    report.evidence.functional_passed = outcome.is_ok();
    report.evidence.error = outcome.as_ref().err().map(ToString::to_string);
    for workspace in fixtures.iter().rev() {
        let stopped = workspace.stop().await.is_ok();
        let mut cleanup = Cleanup {
            workspace: workspace.reference(),
            stop_requested: stopped,
            deletion: None,
            error: None,
        };
        match workspace.destroy().await {
            Ok(mut deletion) => {
                let started = Instant::now();
                while !matches!(
                    deletion.state,
                    DeleteState::Completed | DeleteState::Blocked
                ) && started.elapsed() < Duration::from_secs(60)
                {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    match workspace.inspect_delete(&deletion.reference).await {
                        Ok(value) => deletion = value,
                        Err(error) => {
                            cleanup.error = Some(error.to_string());
                            break;
                        }
                    }
                }
                cleanup.deletion = Some(deletion);
            }
            Err(error) => cleanup.error = Some(error.to_string()),
        }
        report.evidence.cleanup.push(cleanup);
        report.save()?;
    }
    report.evidence.cleanup_confirmed = !fixtures.is_empty()
        && report.evidence.cleanup.iter().all(|entry| {
            entry
                .deletion
                .as_ref()
                .is_some_and(|op| op.state == DeleteState::Completed)
        });
    report.save()?;
    outcome
}
async fn open(
    sdk: &WorkspaceSdk,
    root: &str,
    report: &mut Report,
    fixtures: &mut Vec<Workspace>,
) -> Result<Workspace> {
    let id = WorkspaceId::generate();
    report.evidence.fixtures.push(Fixture {
        requested_id: id.clone(),
        reference: None,
    });
    report.save()?;
    let mut options = SandboxOpenOptions::new(root);
    options.ttl_seconds = Some(900);
    let workspace = sdk
        .open(&ProviderId::new("boat")?, OpenRequest::new(id, &options)?)
        .await?;
    report
        .evidence
        .fixtures
        .last_mut()
        .expect("just recorded fixture")
        .reference = Some(workspace.reference());
    fixtures.push(workspace.clone());
    report.save()?;
    Ok(workspace)
}
async fn exercise(
    sdk: &WorkspaceSdk,
    sync: &SyncService,
    local: &Path,
    workload: &Path,
    root: &str,
    report: &mut Report,
    fixtures: &mut Vec<Workspace>,
) -> Result<()> {
    let source = open(sdk, root, report, fixtures).await?;
    let destination = open(sdk, root, report, fixtures).await?;
    report.passed("native_supervisor_upload_and_workspace_creation")?;
    let project = local.join("project");
    fs::create_dir(&project)?;
    fs::copy(workload, project.join("worker"))?;
    fs::write(project.join("counter"), "0")?;
    fs::write(project.join("local.txt"), "base local\n")?;
    fs::write(project.join("remote.txt"), "base remote\n")?;
    fs::write(project.join(".env"), "DUMMY_SECRET=excluded")?;
    let session = OperationId::generate();
    sync.initialize(session.clone(), &project, &source, true)
        .await?;
    ensure(
        source
            .read_file(ReadFile {
                path: WorkspacePath::new(".env")?,
                offset: 0,
                limit: 1024,
            })
            .await
            .is_err(),
        "secret file was transferred",
    )?;
    report.passed("native_filtered_tree_initialization")?;
    let binary: Vec<u8> = (0..320 * 1024).map(|index| (index % 256) as u8).collect();
    for (index, chunk) in binary.chunks(128 * 1024).enumerate() {
        source
            .write_file(WriteFile {
                path: WorkspacePath::new("binary.dat")?,
                offset: (index * 128 * 1024) as u64,
                bytes: chunk.to_vec(),
                truncate: index == 0,
                executable: false,
            })
            .await?;
    }
    let mut received = Vec::new();
    loop {
        let chunk = source
            .read_file(ReadFile {
                path: WorkspacePath::new("binary.dat")?,
                offset: received.len() as u64,
                limit: 128 * 1024,
            })
            .await?;
        received.extend(chunk.bytes);
        if chunk.eof {
            break;
        }
    }
    ensure(received == binary, "binary transfer changed bytes")?;
    report.passed("native_binary_chunk_roundtrip")?;
    let run = source
        .start(CommandSpec::new("./worker").args(["counter", "counter", "3", "0.1"]))
        .await?;
    let reference = run.reference();
    drop(run);
    let reconnected = sdk.connect(&source.reference()).await?;
    let run = reconnected.run(reference.id);
    ensure(
        run.wait(Duration::from_secs(60)).await? == RunState::Exited { exit_code: 0 },
        "Rust worker failed",
    )?;
    report.passed("rust_workload_reconnect")?;
    fs::write(project.join("local.txt"), "local edit\n")?;
    let result = source
        .exec(CommandSpec::new("./worker").args(["writefile", "remote.txt", "remote edit\n"]))
        .await?;
    ensure(result.success(), "remote edit failed")?;
    let review = sync.sync_back(&session, &source, true).await?;
    ensure(
        review.conflicts.is_empty(),
        "independent edits unexpectedly conflicted",
    )?;
    sync.apply_sync(
        &session,
        &review,
        ApplyOptions {
            quiescent: true,
            expected_fingerprint: review.expected_fingerprint.clone(),
        },
    )
    .await?;
    ensure(
        fs::read_to_string(project.join("local.txt"))? == "local edit\n"
            && fs::read_to_string(project.join("remote.txt"))? == "remote edit\n",
        "three-way publication lost edits",
    )?;
    report.passed("native_three_way_return_sync")?;
    let handoff = sync
        .handoff(
            OperationId::generate(),
            &source,
            &run,
            &destination,
            CommandSpec::new("./worker")
                .args(["counter", "counter", "5", "0.1"])
                .into(),
            true,
        )
        .await?;
    destination
        .run(
            handoff
                .destination_run
                .ok_or_else(|| Error::State("handoff omitted run".into()))?
                .id,
        )
        .wait(Duration::from_secs(60))
        .await?;
    let result = destination
        .exec(CommandSpec::new("./worker").args(["echo", "handoff-complete"]))
        .await?;
    ensure(
        result.success() && result.stdout.contains("handoff-complete"),
        "destination workload failed",
    )?;
    let counter = destination
        .read_file(ReadFile {
            path: WorkspacePath::new("counter")?,
            offset: 0,
            limit: 32,
        })
        .await?;
    ensure(
        counter.bytes == b"5",
        "handoff did not resume portable state",
    )?;
    report.passed("native_box_to_box_handoff")?;
    let active = destination
        .start(CommandSpec::new("./worker").args(["sleep", "30"]))
        .await?;
    ensure(
        active.cancel().await? == RunState::Canceled,
        "managed cancellation not confirmed",
    )?;
    let _ = active.output(OutputCursor::default(), 1024).await?;
    report.passed("native_managed_cancellation")?;
    Ok(())
}
fn ensure(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::State(message.into()))
    }
}
