use super::{
    filesystem::{self, atomic, check, lock, read_json, working_directory, write_json},
    pipe_control,
    protocol::{SupervisorContext, SupervisorExecution},
    run_record::{self, ProcessIdentity, RunClaim, MAX_OUTPUT},
};
use crate::{Error, Ownership, Result, RunId, RunState};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Error as IoError, ErrorKind, Read, Write},
    mem::MaybeUninit,
    os::{
        fd::{AsRawFd, RawFd},
        unix::{
            fs::OpenOptionsExt,
            process::{CommandExt, ExitStatusExt},
        },
    },
    path::{Path, PathBuf},
    process::{self, Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

fn valid_execution(execution: &SupervisorExecution) -> Result<()> {
    execution.command.validate()?;
    if execution.private_stdin.is_some() && execution.command.stdin.is_some() {
        return Err(Error::InvalidInput(
            "provide either ordinary stdin or private stdin".into(),
        ));
    }
    for (key, value) in &execution.secrets.0 {
        if key.is_empty() || key.contains(['\0', '=']) || value.contains('\0') {
            return Err(Error::InvalidInput(
                "invalid secret environment value".into(),
            ));
        }
    }
    Ok(())
}
pub fn start(
    context: &SupervisorContext,
    id: &RunId,
    execution: &SupervisorExecution,
    limit: u64,
) -> Result<RunState> {
    start_mode(context, id, execution, limit, None, false, false)
}
fn validate_piped(execution: &SupervisorExecution) -> Result<()> {
    if execution.command.stdin.is_some() || execution.private_stdin.is_some() {
        return Err(Error::InvalidInput(
            "piped launch requires no launch stdin".into(),
        ));
    }
    Ok(())
}
pub fn start_piped(
    context: &SupervisorContext,
    id: &RunId,
    execution: &SupervisorExecution,
    limit: u64,
) -> Result<RunState> {
    validate_piped(execution)?;
    start_mode(context, id, execution, limit, None, true, false)
}
pub fn start_piped_with_clean_environment(
    context: &SupervisorContext,
    id: &RunId,
    execution: &SupervisorExecution,
    limit: u64,
) -> Result<RunState> {
    if !Path::new(&execution.command.program).is_absolute() {
        return Err(Error::InvalidInput(
            "clean environment requires an absolute executable".into(),
        ));
    }
    validate_piped(execution)?;
    start_mode(context, id, execution, limit, None, true, true)
}
pub(super) fn start_terminal(
    context: &SupervisorContext,
    id: &RunId,
    execution: &SupervisorExecution,
    limit: u64,
    size: crate::TerminalSize,
) -> Result<RunState> {
    if !(2..=128 * 1024).contains(&limit)
        || execution.command.stdin.is_some()
        || execution.private_stdin.is_some()
    {
        return Err(Error::InvalidInput(
            "terminal launch requires 2 bytes..=128 KiB retention and no launch stdin".into(),
        ));
    }
    start_mode(context, id, execution, limit, Some(size), false, false)
}
fn start_mode(
    context: &SupervisorContext,
    id: &RunId,
    execution: &SupervisorExecution,
    limit: u64,
    terminal: Option<crate::TerminalSize>,
    piped: bool,
    clear_environment: bool,
) -> Result<RunState> {
    valid_execution(execution)?;
    if !(2..=MAX_OUTPUT).contains(&limit) {
        return Err(Error::InvalidInput(
            "run output retention must be 2 bytes..=16 MiB".into(),
        ));
    }
    let _workspace_lock = lock(&Path::new(&context.control).join("workspace.lock"))?;
    check(context)?;
    let cwd = working_directory(context, &execution.directory)?;
    let path = run_record::run_directory(context, id, true)?;
    let salt_path = Path::new(&context.control).join("fingerprint-salt");
    if !salt_path.exists() {
        let mut salt = [0; 32];
        File::open("/dev/urandom")?.read_exact(&mut salt)?;
        atomic(&salt_path, &salt)?;
    }
    let mut salt = Vec::new();
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&salt_path)?
        .take(33)
        .read_to_end(&mut salt)?;
    if salt.len() != 32 {
        return Err(Error::State(
            "workspace fingerprint salt is malformed".into(),
        ));
    }
    let mut hash = Sha256::new();
    hash.update(salt);
    // Only the new clean mode adds this tag; all earlier modes retain their
    // exact salted fingerprint encoding and continue reconnecting unchanged.
    if clear_environment {
        hash.update(b"workspace-sdk-clean-env-v1");
    }
    // Preserve exact legacy ordinary/terminal fingerprint encoding.
    if piped {
        hash.update(b"workspace-sdk-piped-v1");
    }
    hash.update(serde_json::to_vec(&(execution, limit, terminal))?);
    let fingerprint = format!("{:x}", hash.finalize());
    let _launch_lock = lock(&path.join("launch.lock"))?;
    let claim_path = path.join("claim.json");
    if claim_path.exists() {
        let claim: RunClaim = read_json(&claim_path)?;
        if claim.version != 1 || claim.fingerprint != fingerprint || claim.piped != piped {
            return Err(Error::Conflict(
                "run ID was already claimed for a different execution".into(),
            ));
        }
        return run_record::inspect(&path);
    }
    let active = lock(&path.join("active.lock"))?;
    write_json(&path.join("status.json"), &RunState::Starting)?;
    write_json(
        &claim_path,
        &RunClaim {
            version: 1,
            fingerprint,
            piped,
        },
    )?;
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(IoError::last_os_error().into());
    }
    if pid > 0 {
        return Ok(RunState::Starting);
    }
    // This executable is single-threaded before fork. The child owns the live
    // lock and detaches transport descriptors before executing the workload.
    unsafe {
        libc::setsid();
    }
    if let Ok(null) = OpenOptions::new().read(true).write(true).open("/dev/null") {
        for fd in [0, 1, 2] {
            unsafe {
                libc::dup2(null.as_raw_fd(), fd);
            }
        }
    }
    for fd in 3..1024 {
        if fd != active.as_raw_fd() {
            unsafe {
                libc::close(fd);
            }
        }
    }
    let result = match terminal {
        Some(size) => super::terminal::work(&path, &cwd, id, execution, limit as usize, size),
        None => work(
            &path,
            &cwd,
            id,
            execution,
            limit as usize,
            piped,
            clear_environment,
        ),
    };
    if result.is_err() {
        let _ = write_json(
            &path.join("status.json"),
            &RunState::Lost {
                reason: "target supervisor could not complete the launch or observation".into(),
            },
        );
    }
    // Inherited Rust guards refer to descriptors closed above; do not unwind
    // them after descriptor numbers have been reused by the detached process.
    unsafe { libc::_exit(0) }
}
fn nonblocking(fd: RawFd) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(IoError::last_os_error().into());
    }
    Ok(())
}
struct OwnedProcessGroup {
    child: Child,
    can_signal: bool,
}
impl OwnedProcessGroup {
    fn signal(&self, signal: i32) {
        if self.can_signal {
            unsafe {
                libc::killpg(self.child.id() as i32, signal);
            }
        }
    }
    fn exit_observed(&mut self) -> Result<bool> {
        let mut info = MaybeUninit::<libc::siginfo_t>::zeroed();
        // WNOWAIT retains the leader's PID until every group signal is sent.
        // Reaping it here would allow the kernel to reuse its process-group ID.
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                self.child.id(),
                info.as_mut_ptr(),
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result < 0 {
            let error = IoError::last_os_error();
            if error.kind() == ErrorKind::Interrupted {
                return Ok(false);
            }
            if error.raw_os_error() == Some(libc::ECHILD) {
                self.can_signal = false;
            }
            return Err(error.into());
        }
        Ok(unsafe { info.assume_init().si_pid() } != 0)
    }
    fn finish(&mut self) -> Result<ExitStatus> {
        self.signal(libc::SIGKILL);
        let result = self.child.wait();
        // Never signal the numeric process group again after reaping was tried.
        self.can_signal = false;
        Ok(result?)
    }
}
impl Drop for OwnedProcessGroup {
    fn drop(&mut self) {
        if self.can_signal {
            let _ = self.finish();
        }
    }
}
fn work(
    path: &Path,
    cwd: &Path,
    id: &RunId,
    execution: &SupervisorExecution,
    limit: usize,
    piped: bool,
    clear_environment: bool,
) -> Result<()> {
    let _directory = if piped {
        Some(super::control_channel::DirectoryGuard::enter(path)?)
    } else {
        None
    };
    let listener = if piped {
        Some(pipe_control::listen()?)
    } else {
        None
    };
    let started = Instant::now();
    let input = execution
        .private_stdin
        .as_ref()
        .map(|value| value.expose())
        .or(execution.command.stdin.as_deref());
    let mut command = Command::new(&execution.command.program);
    command
        .args(&execution.command.args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(if input.is_some() || piped {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .process_group(0);
    if clear_environment {
        command.env_clear();
    }
    for key in &execution.command.env_remove {
        command.env_remove(key);
    }
    command
        .envs(&execution.command.env)
        .envs(&execution.secrets.0);
    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            // A missing cwd is an observation failure, not proof that the
            // requested executable is absent. Runtime probes rely on 127.
            let exit_code = match error.kind() {
                ErrorKind::NotFound if fs::metadata(cwd).is_ok_and(|value| value.is_dir()) => 127,
                ErrorKind::PermissionDenied => 126,
                _ if error.raw_os_error() == Some(libc::ENOEXEC) => 126,
                _ => return Err(error.into()),
            };
            let message = if exit_code == 127 {
                b"workspace-supervisor: executable was not found\n".as_slice()
            } else {
                b"workspace-supervisor: executable could not be executed\n".as_slice()
            };
            run_record::append(&path.join("stderr.log"), message, (limit / 2).max(1))?;
            write_json(&path.join("status.json"), &RunState::Exited { exit_code })?;
            return Ok(());
        }
    };
    let mut group = OwnedProcessGroup {
        child,
        can_signal: true,
    };
    write_json(&path.join("status.json"), &RunState::Running)?;
    write_json(
        &path.join("process.json"),
        &ProcessIdentity {
            supervisor_pid: process::id() as i32,
            pid: group.child.id() as i32,
        },
    )?;
    let mut stdout = group.child.stdout.take();
    let mut stderr = group.child.stderr.take();
    let mut stdin = group.child.stdin.take();
    for fd in [
        stdout.as_ref().map(AsRawFd::as_raw_fd),
        stderr.as_ref().map(AsRawFd::as_raw_fd),
        stdin.as_ref().map(AsRawFd::as_raw_fd),
    ]
    .into_iter()
    .flatten()
    {
        nonblocking(fd)?;
    }
    let bytes = input.unwrap_or(&[]);
    let mut written = 0;
    if bytes.is_empty() && !piped {
        stdin = None;
    }
    let mut exited = false;
    let mut terminal = None;
    let mut terminated: Option<Instant> = None;
    while stdout.is_some() || stderr.is_some() || !exited {
        if let Some(listener) = &listener {
            pipe_control::serve_one(listener, id, &mut stdin)?;
        }
        if terminal.is_none() && path.join("cancel").exists() {
            terminal = Some(RunState::Canceled);
            terminated = Some(Instant::now());
            group.signal(libc::SIGTERM);
        }
        if terminal.is_none()
            && execution
                .command
                .timeout_seconds
                .is_some_and(|seconds| started.elapsed() >= Duration::from_secs(seconds))
        {
            terminal = Some(RunState::TimedOut);
            terminated = Some(Instant::now());
            group.signal(libc::SIGTERM);
        }
        if terminated.is_some_and(|instant| instant.elapsed() >= Duration::from_millis(500)) {
            group.signal(libc::SIGKILL);
        }
        let mut descriptors = vec![];
        for fd in [
            stdout.as_ref().map(AsRawFd::as_raw_fd),
            stderr.as_ref().map(AsRawFd::as_raw_fd),
        ]
        .into_iter()
        .flatten()
        {
            descriptors.push(libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            });
        }
        if let Some(stream) = stdin.as_ref().filter(|_| !piped) {
            descriptors.push(libc::pollfd {
                fd: stream.as_raw_fd(),
                events: libc::POLLOUT,
                revents: 0,
            });
        }
        let polled = unsafe {
            libc::poll(
                descriptors.as_mut_ptr(),
                descriptors.len() as libc::nfds_t,
                50,
            )
        };
        if polled < 0 && IoError::last_os_error().kind() != ErrorKind::Interrupted {
            return Err(IoError::last_os_error().into());
        }
        let mut chunk = [0; 65536];
        if let Some(stream) = stdout.as_mut() {
            match stream.read(&mut chunk) {
                Ok(0) => stdout = None,
                Ok(size) => run_record::append(
                    &path.join("stdout.log"),
                    &chunk[..size],
                    (limit / 2).max(1),
                )?,
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
                Err(e) => return Err(e.into()),
            }
        }
        if let Some(stream) = stderr.as_mut() {
            match stream.read(&mut chunk) {
                Ok(0) => stderr = None,
                Ok(size) => run_record::append(
                    &path.join("stderr.log"),
                    &chunk[..size],
                    (limit / 2).max(1),
                )?,
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
                Err(e) => return Err(e.into()),
            }
        }
        if let Some(stream) = stdin.as_mut().filter(|_| !piped) {
            match stream.write(&bytes[written..(written + 65536).min(bytes.len())]) {
                Ok(size) => {
                    written += size;
                    if written == bytes.len() {
                        stdin = None;
                    }
                }
                Err(e) if e.kind() == ErrorKind::BrokenPipe => stdin = None,
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
                Err(e) => return Err(e.into()),
            }
        }
        if !exited {
            exited = group.exit_observed()?;
        }
        if exited && terminated.is_none() {
            group.signal(libc::SIGTERM);
            terminated = Some(Instant::now());
        }
    }
    // Finalize descendants and reap the retained leader before publishing a
    // terminal state that permits the project to be checkpointed or handed off.
    let status = group.finish()?;
    // A terminal receipt must never outlive this owner's input writer. Close it
    // explicitly before publishing completion, including a still-open pipe mode.
    drop(stdin);
    let code = status
        .code()
        .unwrap_or_else(|| -status.signal().unwrap_or(1));
    write_json(
        &path.join("status.json"),
        &terminal.unwrap_or(RunState::Exited { exit_code: code }),
    )?;
    Ok(())
}
pub fn cancel(path: &Path) -> Result<RunState> {
    let mut state = run_record::inspect(path)?;
    if state.is_active() {
        atomic(&path.join("cancel"), b"cancel")?;
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            state = run_record::inspect(path)?;
            if !state.is_active() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
    Ok(state)
}
pub fn stop_owned(context: &SupervisorContext, destroy: bool) -> Result<()> {
    if context.ownership != Ownership::SdkOwned {
        return Err(Error::Unsupported(
            "attached workspace lifecycle cannot be mutated".into(),
        ));
    }
    let _lock = lock(&Path::new(&context.control).join("workspace.lock"))?;
    check(context)?;
    let mut paths = Vec::<PathBuf>::new();
    for entry in fs::read_dir(Path::new(&context.control).join("runs"))? {
        let entry = entry?;
        let id = RunId::new(
            entry
                .file_name()
                .into_string()
                .map_err(|_| Error::State("invalid private run directory".into()))?,
        )?;
        let path = run_record::run_directory(context, &id, false)?;
        if !path.join("claim.json").exists() {
            continue;
        }
        let state = run_record::inspect(&path)?;
        if matches!(state, RunState::Lost { .. }) {
            return Err(Error::State(
                "a lost run must be reconciled before workspace lifecycle mutation".into(),
            ));
        }
        if state.is_active() {
            atomic(&path.join("cancel"), b"cancel")?;
        }
        paths.push(path);
    }
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let mut active = false;
        for path in &paths {
            active |= filesystem::live_lock(&path.join("active.lock"))?;
        }
        if !active {
            break;
        }
        if Instant::now() >= until {
            return Err(Error::State("managed processes have not stopped".into()));
        }
        thread::sleep(Duration::from_millis(50));
    }
    if destroy {
        fs::remove_dir_all(&context.root)?;
        fs::remove_dir_all(&context.control)?;
    }
    Ok(())
}
