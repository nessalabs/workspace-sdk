//! Convert validated SDK launch data at the process boundary.
use super::protocol::SupervisorExecution;
use crate::{Error, Result};
use pty_runtime::{CommandSpec, EnvironmentPolicy};
use std::{
    ffi::OsString,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

pub(super) fn command(cwd: &Path, execution: &SupervisorExecution) -> Result<CommandSpec> {
    let path = resolve(cwd, execution)?;
    let overrides = execution
        .command
        .env
        .iter()
        .chain(execution.secrets.0.iter())
        .map(|(key, value)| (OsString::from(key), OsString::from(value)))
        .collect();
    CommandSpec::new(
        path,
        cwd.to_owned(),
        execution.command.args.iter().map(OsString::from).collect(),
    )
    .and_then(|command| {
        command.with_environment(
            EnvironmentPolicy::Inherit,
            execution
                .command
                .env_remove
                .iter()
                .map(OsString::from)
                .collect(),
            overrides,
        )
    })
    .map_err(|_| Error::InvalidInput("terminal command exceeds the native command bounds".into()))
}
fn resolve(cwd: &Path, execution: &SupervisorExecution) -> Result<PathBuf> {
    let program = Path::new(&execution.command.program);
    if program.is_absolute() {
        return Ok(program.to_owned());
    }
    if program.components().count() > 1 {
        return Ok(cwd.join(program));
    }
    let inherited = (!execution.command.env_remove.iter().any(|key| key == "PATH"))
        .then(|| std::env::var("PATH").ok())
        .flatten();
    let path = execution
        .secrets
        .0
        .get("PATH")
        .or(execution.command.env.get("PATH"))
        .cloned()
        .or(inherited)
        .unwrap_or_else(|| "/usr/bin:/bin".into());
    for directory in std::env::split_paths(&path) {
        let candidate = if directory.is_absolute() {
            directory.join(program)
        } else {
            cwd.join(directory).join(program)
        };
        let encoded = std::ffi::CString::new(candidate.as_os_str().as_bytes())
            .map_err(|_| Error::InvalidInput("terminal search path contains NUL".into()))?;
        // SAFETY: the NUL-terminated path lives through the call; effective IDs
        // are the credentials used by the subsequent process launch.
        let executable = unsafe {
            libc::faccessat(
                libc::AT_FDCWD,
                encoded.as_ptr(),
                libc::X_OK,
                libc::AT_EACCESS,
            )
        } == 0;
        if candidate.is_file() && executable {
            return Ok(candidate);
        }
    }
    Err(Error::NotFound("terminal executable".into()))
}
