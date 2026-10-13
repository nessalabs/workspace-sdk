//! Linux probe: pty_live_probe <start|reconnect|cleanup> <absolute-state-root> <absolute-supervisor>.
//! Run each phase in a separate remote RPC. The state root must not exist at start.
//! This exercises LocalProvider inside the selected machine, not a cloud-provider adapter.
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env,
    fs::{self, DirBuilder, OpenOptions},
    io::{self, BufRead, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
    sync::Arc,
    thread,
    time::Duration,
};
use tokio::time::{sleep, timeout};
use workspace_sdk::{
    providers::{LocalConfig, LocalProvider},
    runtime::*,
    BackendDescriptor, DeleteState, IProvider, OpenRequest, OutputCursor, SecretInput, WorkspaceId,
};
type ProbeResult<T> = Result<T, Box<dyn std::error::Error>>;
const KEY: &str = "live-pty-probe";
const PREFIX: &str = "tty:111\nforeground:yes\nsize:80x24\nready\n";
const CHALLENGE: &[u8] = b"probe-private-challenge-7f91\n";

#[derive(Clone, Serialize, Deserialize)]
struct Identity {
    pid: u32,
    start_ticks: String,
    boot_id: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct State {
    phase: String,
    supervisor: PathBuf,
    executable: PathBuf,
    descriptor: BackendDescriptor,
    process: Option<RuntimeProcessRef>,
    cursor: OutputCursor,
    transcript: String,
    identity: Option<Identity>,
    owner: Option<Identity>,
    owner_lock: Option<PathBuf>,
}
fn watchdog(seconds: u64) {
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(seconds));
        eprintln!("probe deadline exceeded");
        process::exit(124);
    });
}
fn proc_identity(pid: u32) -> ProbeResult<(String, String)> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let fields: Vec<_> = stat
        .rsplit_once(") ")
        .ok_or("invalid process stat")?
        .1
        .split_whitespace()
        .collect();
    Ok((
        fields.first().ok_or("missing process state")?.to_string(),
        fields
            .get(19)
            .ok_or("missing process start time")?
            .to_string(),
    ))
}
fn size() -> ProbeResult<String> {
    let output = Command::new("/bin/stty")
        .arg("size")
        .stdin(Stdio::inherit())
        .output()?;
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout)?;
    let dimensions: Vec<_> = text.split_whitespace().collect();
    assert_eq!(dimensions.len(), 2);
    Ok(format!("size:{}x{}\n", dimensions[1], dimensions[0]))
}
fn emit(text: &str) -> ProbeResult<()> {
    let mut output = io::stdout().lock();
    output.write_all(text.as_bytes())?;
    output.flush()?;
    Ok(())
}
fn workload() -> ProbeResult<()> {
    watchdog(600);
    // SAFETY: these calls only query this process's own terminal descriptors/group.
    unsafe {
        assert_eq!(
            (libc::isatty(0), libc::isatty(1), libc::isatty(2)),
            (1, 1, 1)
        );
        assert_eq!(libc::tcgetpgrp(0), libc::getpgrp());
    }
    assert!(Command::new("/bin/stty")
        .args(["-echo", "-icanon", "min", "1", "time", "0", "-opost"])
        .stdin(Stdio::inherit())
        .status()?
        .success());
    let mut launches = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open("launches")?;
    launches.write_all(b"launch\n")?;
    launches.sync_all()?;
    let identity = Identity {
        pid: process::id(),
        start_ticks: proc_identity(process::id())?.1,
        boot_id: fs::read_to_string("/proc/sys/kernel/random/boot_id")?,
    };
    let mut record = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open("workload-identity.json")?;
    record.write_all(&serde_json::to_vec(&identity)?)?;
    record.sync_all()?;
    emit("tty:111\nforeground:yes\n")?;
    emit(&size()?)?;
    emit("ready\n")?;
    for line in io::stdin().lock().lines() {
        match line?.as_str() {
            "probe-private-challenge-7f91" => emit("input:accepted\n")?,
            "size" => emit(&size()?)?,
            _ => return Err("unexpected workload input".into()),
        }
    }
    Err("unexpected workload EOF".into())
}
fn save(root: &Path, state: &State) -> ProbeResult<()> {
    // Fresh 0600 temporary names make a prior interrupted save harmless.
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    file.write_all(&serde_json::to_vec_pretty(state)?)?;
    file.as_file().sync_all()?;
    file.persist(root.join("state.json"))?;
    fs::File::open(root)?.sync_all()?;
    Ok(())
}
fn owner_identity(descriptor: &BackendDescriptor) -> ProbeResult<(Identity, PathBuf)> {
    // Supplemental Linux/process evidence, never used to signal a cached PID.
    let runs = fs::read_dir(Path::new(&descriptor.control_root).join("runs"))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(runs.len(), 1, "this probe creates exactly one managed run");
    let run = runs[0].path();
    let record: serde_json::Value = serde_json::from_slice(&fs::read(run.join("process.json"))?)?;
    let pid = u32::try_from(
        record["supervisor_pid"]
            .as_u64()
            .ok_or("missing owner PID")?,
    )?;
    Ok((
        Identity {
            pid,
            start_ticks: proc_identity(pid)?.1,
            boot_id: fs::read_to_string("/proc/sys/kernel/random/boot_id")?,
        },
        run.join("active.lock"),
    ))
}
fn stopped(identity: &Identity) -> ProbeResult<bool> {
    match proc_identity(identity.pid) {
        Ok((status, ticks)) => Ok(ticks != identity.start_ticks || status == "Z"),
        Err(error)
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::NotFound) =>
        {
            Ok(true)
        }
        Err(error) => Err(error),
    }
}
fn command(state: &State) -> RuntimeCommand {
    RuntimeCommand {
        program: state.executable.to_str().unwrap().into(),
        args: vec!["--workload".into()],
        working_directory: state.descriptor.root.clone(),
        environment: BTreeMap::new(),
        secret_environment: BTreeMap::new(),
        remove_environment: Vec::new(),
        timeout_seconds: 600,
    }
}
async fn collect(
    host: &BackendRuntimeHost,
    state: &mut State,
    expected: &str,
    complete: bool,
) -> ProbeResult<()> {
    loop {
        let page = host
            .read_process_output(
                state.process.as_ref().ok_or("missing process")?,
                state.cursor.clone(),
                4096,
            )
            .await?;
        assert!(!page.gap, "small probe transcript must be complete");
        assert!(page.stderr.is_empty());
        assert_eq!(page.next.stderr, 0);
        assert_eq!(
            page.next.stdout,
            state.cursor.stdout + page.stdout.len() as u64
        );
        state.transcript.push_str(&page.stdout);
        state.cursor = page.next;
        assert!(
            expected.starts_with(&state.transcript),
            "unexpected transcript or echoed challenge"
        );
        if state.transcript == expected && (!complete || page.complete) {
            return Ok(());
        }
        assert!(!page.complete, "premature completion");
        sleep(Duration::from_millis(20)).await;
    }
}
async fn submit(
    host: &BackendRuntimeHost,
    process: &RuntimeProcessRef,
    bytes: &[u8],
) -> ProbeResult<()> {
    assert_eq!(
        host.write_process_input(process, &SecretInput::new(bytes.to_vec()))
            .await?,
        RuntimeInputOutcome::Accepted { bytes: bytes.len() }
    );
    Ok(())
}
async fn verify_cleanup(host: &BackendRuntimeHost, state: &mut State) -> ProbeResult<()> {
    assert!(
        matches!(state.phase.as_str(), "started" | "reconnected"),
        "previous phase never completed"
    );
    let process = state.process.clone().ok_or("missing process")?;
    let expected = state.transcript.clone();
    collect(host, state, &expected, true).await?;
    assert_eq!(
        host.inspect_process(&process).await?,
        RuntimeProcessState::Exited { code: 130 }
    );
    let end = host
        .read_process_output(&process, state.cursor.clone(), 4096)
        .await?;
    assert!(end.complete && !end.gap && end.stdout.is_empty() && end.stderr.is_empty());
    assert_eq!(end.next.stdout, state.cursor.stdout);
    assert_eq!(end.next.stderr, state.cursor.stderr);
    assert!(
        stopped(state.identity.as_ref().ok_or("missing workload identity")?)?,
        "workload still running"
    );
    let owner = state.owner.as_ref().ok_or("missing owner identity")?;
    if !stopped(owner)? {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(state.owner_lock.as_ref().ok_or("missing owner lock")?)?;
        lock.try_lock_exclusive()?;
    }
    Ok(())
}
async fn controller(phase: &str, root: &Path, supervisor: PathBuf) -> ProbeResult<()> {
    if phase == "start" {
        DirBuilder::new().mode(0o700).create(root)?;
    }
    let metadata = fs::symlink_metadata(root)?;
    // SAFETY: geteuid reads the current effective user identity.
    assert!(metadata.is_dir() && metadata.uid() == unsafe { libc::geteuid() });
    assert_eq!(metadata.mode() & 0o777, 0o700);
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(root.join("controller.lock"))?;
    lock.try_lock_exclusive()?;
    let mut config = LocalConfig::trusted_host(root.join("workspaces"));
    config.supervisor = supervisor.clone();
    let provider = LocalProvider::new(config)?;
    let mut state = if phase == "start" {
        let backend = provider
            .open(OpenRequest {
                workspace_id: WorkspaceId::generate(),
                config: Default::default(),
                resources: Default::default(),
            })
            .await?;
        let state = State {
            phase: "created".into(),
            supervisor: supervisor.clone(),
            executable: fs::canonicalize(env::current_exe()?)?,
            descriptor: backend.descriptor(),
            process: None,
            cursor: OutputCursor::default(),
            transcript: String::new(),
            identity: None,
            owner: None,
            owner_lock: None,
        };
        save(root, &state)?;
        state
    } else {
        let path = root.join("state.json");
        let metadata = fs::symlink_metadata(&path)?;
        assert!(metadata.is_file());
        assert_eq!(metadata.mode() & 0o777, 0o600);
        serde_json::from_slice(&fs::read(path)?)?
    };
    assert_eq!(state.supervisor, supervisor);
    assert_eq!(state.executable, fs::canonicalize(env::current_exe()?)?);
    let backend = provider.connect(&state.descriptor.reference).await?;
    assert_eq!(backend.descriptor(), state.descriptor);
    let host = BackendRuntimeHost::new(backend);
    let mut cleanup_failure = None;
    match phase {
        "start" => {
            state.process = Some(host.start_interactive(KEY, command(&state), true).await?);
            save(root, &state)?;
            collect(&host, &mut state, PREFIX, false).await?;
            assert_eq!(
                fs::read(Path::new(&state.descriptor.root).join("launches"))?,
                b"launch\n"
            );
            state.identity = Some(serde_json::from_slice(&fs::read(
                Path::new(&state.descriptor.root).join("workload-identity.json"),
            )?)?);
            assert_eq!(
                host.inspect_process(state.process.as_ref().unwrap())
                    .await?,
                RuntimeProcessState::Running
            );
            let (owner, owner_lock) = owner_identity(&state.descriptor)?;
            state.owner = Some(owner);
            state.owner_lock = Some(owner_lock);
            state.phase = "started".into();
        }
        "reconnect" => {
            assert_eq!(state.phase, "started");
            assert_eq!(state.transcript, PREFIX);
            assert_eq!(state.cursor.stdout, PREFIX.len() as u64);
            assert_eq!(state.cursor.stderr, 0);
            let process = state.process.clone().ok_or("missing process")?;
            assert_eq!(
                host.inspect_process(&process).await?,
                RuntimeProcessState::Running
            );
            let identity = state.identity.as_ref().ok_or("missing workload identity")?;
            assert_eq!(
                fs::read_to_string("/proc/sys/kernel/random/boot_id")?,
                identity.boot_id
            );
            assert_eq!(proc_identity(identity.pid)?.1, identity.start_ticks);
            let owner = state.owner.as_ref().ok_or("missing owner identity")?;
            assert_eq!(proc_identity(owner.pid)?.1, owner.start_ticks);
            assert!(!stopped(owner)?);
            assert_eq!(
                fs::read_to_string("/proc/sys/kernel/random/boot_id")?,
                owner.boot_id
            );
            assert_eq!(
                host.start_interactive(KEY, command(&state), true).await?,
                process
            );
            assert_eq!(
                fs::read(Path::new(&state.descriptor.root).join("launches"))?,
                b"launch\n"
            );
            let replay = host
                .read_process_output(&process, OutputCursor::default(), 4096)
                .await?;
            assert_eq!(replay.stdout, PREFIX);
            assert!(replay.stderr.is_empty() && !replay.complete && !replay.gap);
            assert_eq!(replay.next.stdout, state.cursor.stdout);
            assert_eq!(replay.next.stderr, state.cursor.stderr);
            submit(&host, &process, CHALLENGE).await?;
            collect(
                &host,
                &mut state,
                &format!("{PREFIX}input:accepted\n"),
                false,
            )
            .await?;
            host.resize_process(&process, RuntimeTerminalSize::new(120, 40)?)
                .await?;
            submit(&host, &process, b"size\n").await?;
            collect(
                &host,
                &mut state,
                &format!("{PREFIX}input:accepted\nsize:120x40\n"),
                false,
            )
            .await?;
            state.phase = "reconnected".into();
        }
        "cleanup" => {
            // Cleanup remains reachable when the previous controller's record lags
            // behind delivery. Verification failures never skip owned destruction.
            let mut failures = Vec::new();
            if let Some(process) = &state.process {
                match timeout(Duration::from_secs(15), host.cancel_process(process)).await {
                    Ok(Ok(RuntimeProcessState::Exited { code: 130 })) => (),
                    other => failures.push(format!("cancellation oracle failed: {other:?}")),
                }
            } else {
                failures.push("prior start did not save a process reference".into());
            }
            let verification_host = BackendRuntimeHost::new(Arc::clone(host.backend()));
            let mut verification_state = state.clone();
            let mut verification = tokio::spawn(async move {
                verify_cleanup(&verification_host, &mut verification_state)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok::<_, String>(verification_state)
            });
            match timeout(Duration::from_secs(30), &mut verification).await {
                Ok(Ok(Ok(verified))) => state = verified,
                Ok(Ok(Err(error))) => {
                    failures.push(format!("cleanup verification failed: {error}"))
                }
                Ok(Err(error)) => failures.push(format!("cleanup verification failed: {error}")),
                Err(_) => {
                    verification.abort();
                    let _ = verification.await;
                    failures.push("cleanup verification deadline exceeded".into());
                }
            }
            let deleted = match timeout(Duration::from_secs(30), host.backend().destroy()).await {
                Ok(Ok(deletion)) if deletion.state == DeleteState::Completed => true,
                other => {
                    failures.push(format!("owned deletion failed: {other:?}"));
                    false
                }
            };
            if deleted {
                for path in [&state.descriptor.root, &state.descriptor.control_root] {
                    match fs::symlink_metadata(path) {
                        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                        other => failures.push(format!(
                            "deleted path still exists or cannot be inspected: {path}: {other:?}"
                        )),
                    }
                }
            }
            state.phase = if failures.is_empty() {
                "cleaned"
            } else {
                "cleanup_failed_verification"
            }
            .into();
            if !failures.is_empty() {
                cleanup_failure = Some(failures.join("; "));
            }
        }
        _ => return Err("expected start, reconnect, or cleanup".into()),
    }
    save(root, &state)?;
    println!(
        "{}",
        serde_json::json!({"phase":state.phase,"controller_pid":process::id(),"workspace":state.descriptor.reference,"process":state.process,"cursor":state.cursor,"workload":state.identity,"owner":state.owner,"transcript":state.transcript})
    );
    if let Some(failure) = cleanup_failure {
        return Err(failure.into());
    }
    Ok(())
}
fn main() -> ProbeResult<()> {
    let args: Vec<_> = env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "--workload") {
        return workload();
    }
    watchdog(110);
    assert_eq!(
        args.len(),
        4,
        "expected phase, absolute state root, absolute supervisor"
    );
    assert!(matches!(
        args[1].as_str(),
        "start" | "reconnect" | "cleanup"
    ));
    let root = PathBuf::from(&args[2]);
    assert!(root.is_absolute());
    let supervisor = PathBuf::from(&args[3]);
    assert!(supervisor.is_absolute());
    let supervisor = fs::canonicalize(supervisor)?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(controller(&args[1], &root, supervisor))
}
