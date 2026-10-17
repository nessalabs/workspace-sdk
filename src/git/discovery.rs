use super::{porcelain, GitWorktreeInventory};
use crate::{CommandSpec, Error, ExecResult, Result, Workspace};

/// Read-only Git discovery anchored to an existing workspace connection.
///
/// Runs one shell-free command through [`Workspace::exec`] with a requested
/// ten-second deadline. The backend owns cancellation and process cleanup; this
/// service does not retry, attach providers, create worktrees, or retain tasks.
/// Host Git configuration and `safe.directory` checks remain in effect.
#[derive(Clone)]
pub struct GitWorktreeDiscovery {
    anchor: Workspace,
    git_executable: String,
}

impl GitWorktreeDiscovery {
    /// Validate the executable structurally using [`CommandSpec::validate`].
    /// Availability and repository access are checked only during discovery.
    pub fn new(anchor: Workspace, git_executable: impl Into<String>) -> Result<Self> {
        let git_executable = git_executable.into();
        discovery_command(&git_executable).validate()?;
        Ok(Self {
            anchor,
            git_executable,
        })
    }

    /// Observe the anchor repository's complete worktree inventory once.
    ///
    /// Accepts at most 256 KiB of captured stdout and stderr combined, 256 records,
    /// 16 KiB per path, and 4 KiB per reason or branch reference. This acceptance
    /// bound applies after execution; capture allocation is the backend's contract.
    /// Requires complete NUL-delimited UTF-8 output without replacement characters.
    /// Nonzero or absent exit status, timeout, truncation, and invalid output
    /// produce a state error with a fixed diagnostic and no partial inventory.
    /// Execution-port errors propagate without retry; Git stderr is never embedded
    /// in this service's diagnostics. Observations convey no access authorization.
    pub async fn discover(&self) -> Result<GitWorktreeInventory> {
        let result = self
            .anchor
            .exec(discovery_command(&self.git_executable))
            .await?;
        let output = checked_output(result)?;
        Ok(GitWorktreeInventory {
            anchor: self.anchor.reference(),
            worktrees: porcelain::parse(&output)?,
        })
    }
}

fn discovery_command(program: &str) -> CommandSpec {
    let mut command = CommandSpec::new(program)
        .args(["--no-pager", "worktree", "list", "--porcelain", "-z"])
        .timeout(10);
    command.env_remove = [
        "GIT_DIR",
        "GIT_COMMON_DIR",
        "GIT_WORK_TREE",
        "GIT_IMPLICIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_REPLACE_REF_BASE",
        "GIT_NO_REPLACE_OBJECTS",
        "GIT_SHALLOW_FILE",
        "GIT_GRAFT_FILE",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
        "GIT_PREFIX",
        "GIT_SUPER_PREFIX",
        "GIT_INTERNAL_SUPER_PREFIX",
        "GIT_CONFIG",
        "GIT_CONFIG_PARAMETERS",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    for key in [
        "GIT_CONFIG_COUNT",
        "GIT_TERMINAL_PROMPT",
        "GIT_OPTIONAL_LOCKS",
    ] {
        command.env.insert(key.into(), "0".into());
    }
    command
}

fn checked_output(result: ExecResult) -> Result<String> {
    if result.exit_code != Some(0)
        || result.timed_out
        || result.output_truncated
        || result.stdout.len().saturating_add(result.stderr.len()) > porcelain::MAX_OUTPUT_BYTES
    {
        return Err(Error::State(
            "Git worktree discovery did not return a complete bounded result".into(),
        ));
    }
    Ok(result.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_is_bounded_argv_and_preserves_host_configuration() {
        let command = discovery_command("/opt/git with spaces");
        assert_eq!(command.program, "/opt/git with spaces");
        assert_eq!(
            command.args,
            ["--no-pager", "worktree", "list", "--porcelain", "-z"]
        );
        assert_eq!(command.timeout_seconds, Some(10));
        assert!(command.stdin.is_none());
        assert_eq!(command.cwd, crate::WorkspacePath::root());
        for key in [
            "GIT_CONFIG_COUNT",
            "GIT_TERMINAL_PROMPT",
            "GIT_OPTIONAL_LOCKS",
        ] {
            assert_eq!(command.env.get(key).map(String::as_str), Some("0"));
        }
        for key in [
            "GIT_DIR",
            "GIT_COMMON_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_NAMESPACE",
            "GIT_CONFIG_PARAMETERS",
        ] {
            assert!(
                command.env_remove.iter().any(|name| name == key),
                "missing {key}"
            );
        }
        for key in [
            "GIT_CONFIG_GLOBAL",
            "GIT_CONFIG_SYSTEM",
            "GIT_CONFIG_NOSYSTEM",
        ] {
            assert!(!command.env.contains_key(key));
            assert!(!command.env_remove.iter().any(|name| name == key));
        }
        command.validate().unwrap();
        assert!(discovery_command("").validate().is_err());
        assert!(discovery_command("git\0bad").validate().is_err());
    }

    #[test]
    fn rejects_ambiguous_failed_truncated_and_oversized_results_without_stderr() {
        for (exit_code, timed_out, output_truncated) in [
            (None, false, false),
            (Some(1), false, false),
            (Some(0), true, false),
            (Some(0), false, true),
        ] {
            let result = ExecResult {
                exit_code,
                timed_out,
                output_truncated,
                stdout: "worktree /r\0bare\0\0".into(),
                stderr: "private diagnostic".into(),
            };
            let error = checked_output(result).unwrap_err();
            assert!(matches!(error, Error::State(_)));
            assert!(!error.to_string().contains("private diagnostic"));
        }
        let oversized = ExecResult {
            exit_code: Some(0),
            timed_out: false,
            output_truncated: false,
            stdout: "x".repeat(super::super::porcelain::MAX_OUTPUT_BYTES + 1),
            stderr: String::new(),
        };
        assert!(checked_output(oversized).is_err());
    }

    #[test]
    fn combined_capture_bound_counts_stderr_and_accepts_exact_limit() {
        let stdout = "worktree /r\0bare\0\0".to_owned();
        let stderr = "x".repeat(porcelain::MAX_OUTPUT_BYTES - stdout.len());
        let mut result = ExecResult {
            exit_code: Some(0),
            timed_out: false,
            output_truncated: false,
            stdout: stdout.clone(),
            stderr,
        };
        assert_eq!(checked_output(result.clone()).unwrap(), stdout);
        result.stderr.push('x');
        assert!(checked_output(result).is_err());
    }
}
