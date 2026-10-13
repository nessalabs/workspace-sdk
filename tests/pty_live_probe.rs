//! Exercise the probe's pre-launch failure through its public executable.
#![cfg(unix)]

use std::{
    fs, io,
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use workspace_sdk::{DeleteOperation, DeleteState};

#[test]
fn initial_state_write_failure_destroys_the_opened_workspace() {
    // Short paths keep the compact deletion receipt below the real OS limit.
    // The pretty initial probe state is larger; no SDK or filesystem mocks run.
    let temporary = tempfile::Builder::new()
        .prefix("probe-")
        .tempdir_in("/tmp")
        .unwrap();
    let root = temporary.path().join("state");
    let executable = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples/pty_live_probe");
    assert!(
        executable.is_file(),
        "build the pty_live_probe example first"
    );
    let mut command = Command::new(executable);
    command
        .arg("start")
        .arg(&root)
        .arg(env!("CARGO_BIN_EXE_workspace-supervisor"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: the child changes only its own signal disposition/resource limit
    // before exec. These libc calls allocate nothing and take no Rust locks.
    unsafe {
        command.pre_exec(|| {
            if libc::signal(libc::SIGXFSZ, libc::SIG_IGN) == libc::SIG_ERR {
                return Err(io::Error::last_os_error());
            }
            let limit = libc::rlimit {
                rlim_cur: 1024,
                rlim_max: 1024,
            };
            if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(45);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("initial-save cleanup exceeded deadline: {output:?}");
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success(), "failed save must remain an error");
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(
        diagnostic.contains("File too large"),
        "preserve the original OS save failure: {diagnostic}"
    );
    assert!(!root.join("state.json").exists(), "initial save must fail");

    let workspaces = root.join("workspaces");
    let receipts = workspaces.join(".deletions");
    assert!(
        receipts.is_dir(),
        "opened workspace must receive owned destruction after save fails: {diagnostic}"
    );
    let receipts = fs::read_dir(receipts)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(
        receipts.len(),
        1,
        "exactly one opened workspace is destroyed"
    );
    let deletion: DeleteOperation =
        serde_json::from_slice(&fs::read(&receipts[0]).unwrap()).unwrap();
    assert_eq!(deletion.state, DeleteState::Completed);
    let owned = workspaces.join(deletion.reference.workspace.id.as_str());
    assert!(!owned.join("project").exists());
    assert!(!owned.join("control").exists());
    assert!(!owned.join("project/launches").exists());
    assert_eq!(
        fs::read_dir(workspaces).unwrap().count(),
        1,
        "only the durable deletion receipt directory may remain"
    );
}
