use super::{
    command::git_command,
    validation::{valid_launch_bounds, valid_worktree_path},
    GitWorktrees,
};
use crate::{CommandSpec, Error, LaunchRequest, Result, Run, RunRef};
use serde::{Deserialize, Serialize};

/// Caller-persisted inputs for one explicitly authorized Git worktree removal.
///
/// Persist every field before launch and reuse the exact request on uncertainty.
/// This data conveys no ownership or authorization. Deserialization does not
/// validate; [`Self::validate`] is repeated before launch.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitWorktreeRemoveRequest {
    /// Existing workspace-qualified run identity, chosen before first launch.
    pub run: RunRef,
    /// Absolute POSIX target-host path, at most 16 KiB, excluding root, empty,
    /// `.` or `..` components, NUL and replacement characters. A locator, not identity.
    pub path: String,
    /// Requested execution deadline, 1 through 3600 seconds; default 120.
    pub timeout_seconds: u64,
    /// Combined retained managed stdout/stderr budget, 2 bytes through 16 MiB;
    /// default 256 KiB. A bounded tail may have gaps exposed by normal run output.
    pub output_limit_bytes: u64,
}

impl GitWorktreeRemoveRequest {
    /// Construct and validate with a 120-second deadline and 256 KiB retention.
    /// Filesystem state, Git availability and removal authorization are not checked.
    pub fn new(run: RunRef, path: impl Into<String>) -> Result<Self> {
        let request = Self {
            run,
            path: path.into(),
            timeout_seconds: 120,
            output_limit_bytes: 256 * 1024,
        };
        request.validate()?;
        Ok(request)
    }

    /// Check bounded target-host path syntax, execution deadline and retention.
    /// This pure validation neither observes a repository nor authorizes deletion.
    pub fn validate(&self) -> Result<()> {
        if !valid_worktree_path(&self.path, false) {
            return Err(Error::InvalidInput(
                "invalid Git worktree removal path".into(),
            ));
        }
        if !valid_launch_bounds(self.timeout_seconds, self.output_limit_bytes) {
            return Err(Error::InvalidInput(
                "Git worktree removal launch bounds exceeded".into(),
            ));
        }
        Ok(())
    }
}

impl GitWorktrees {
    /// Start or reconnect to one explicit, non-force `git worktree remove` run.
    ///
    /// The caller must authorize deletion, stop all target users and processes, and
    /// retain exclusive, quiescent access through terminal resolution or uncertainty.
    /// Discovery or SDK creation supplies no deletion permission. Git's ordinary
    /// dirty, locked, main-worktree and submodule guards remain enabled; the command
    /// sets `status.showUntrackedFiles=all` so host configuration cannot hide ordinary
    /// untracked files. **Ignored files can still be deleted by Git.** Git may leave
    /// partial deletion on errors or signals; this is not a transaction or rollback.
    /// The SDK never forces, unlocks, prunes, resets, or deletes branches.
    ///
    /// Pure descriptor checks reject removal of the anchor root, control root or
    /// their lexical ancestors. Both descriptor paths must be normalized absolute
    /// POSIX paths. These checks cannot detect aliases or symlinks. The caller must
    /// keep the anchor, control storage and common Git repository metadata stable
    /// and physically outside the removal subtree, including through aliases.
    ///
    /// Uses [`Workspace::start_with`](crate::Workspace::start_with) directly without
    /// mutable existence or inventory preflight. Built-in native backends reconnect
    /// the same durable launch claim even after success removes or reuses the path.
    /// Custom backends must honor that managed launch contract. The request must
    /// name this exact workspace. Pure validation and guards may reject earlier;
    /// incompatible reuse reaching an existing launch claim conflicts.
    /// Retry only the exact persisted request, never a fresh ID on an unknown outcome.
    ///
    /// A returned [`Run`] confirms launch handling, not successful removal. Inspect
    /// its state, wait, output and cancellation normally. A wait timeout does not
    /// cancel; transport errors and `Lost` leave the outcome uncertain. Dropping
    /// the future or handle does not cancel. There is no automatic retry or cleanup.
    /// Trusted Git and host configuration retain their authority and can expose
    /// output. This does not change provider ownership or `Workspace::destroy`.
    pub async fn start_remove(&self, request: GitWorktreeRemoveRequest) -> Result<Run> {
        request.validate()?;
        let descriptor = self.anchor.descriptor();
        if request.run.workspace != descriptor.reference {
            return Err(Error::InvalidInput(
                "Git removal run belongs to a different workspace".into(),
            ));
        }
        validate_anchor_paths(&request.path, &descriptor.root, &descriptor.control_root)?;
        self.anchor
            .start_with(LaunchRequest {
                id: request.run.id.clone(),
                execution: remove_command(&self.git_executable, &request).into(),
                output_limit_bytes: request.output_limit_bytes,
            })
            .await
    }
}

fn validate_anchor_paths(target: &str, root: &str, control_root: &str) -> Result<()> {
    for protected in [root, control_root] {
        if !valid_worktree_path(protected, true) {
            return Err(Error::InvalidInput(
                "Git removal requires normalized absolute POSIX descriptor paths".into(),
            ));
        }
        if target == protected
            || protected
                .strip_prefix(target)
                .is_some_and(|suffix| suffix.starts_with('/'))
        {
            return Err(Error::InvalidInput(
                "Git removal target contains anchor or control storage".into(),
            ));
        }
    }
    Ok(())
}

fn remove_command(program: &str, request: &GitWorktreeRemoveRequest) -> CommandSpec {
    git_command(program)
        .args([
            "--no-pager",
            "-c",
            "status.showUntrackedFiles=all",
            "worktree",
            "remove",
            "--",
            &request.path,
        ])
        .timeout(request.timeout_seconds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProviderId, RunId, WorkspaceId, WorkspaceLocator, WorkspaceRef};

    fn request() -> GitWorktreeRemoveRequest {
        GitWorktreeRemoveRequest::new(
            RunRef {
                workspace: WorkspaceRef {
                    version: 1,
                    id: WorkspaceId::new("anchor").unwrap(),
                    provider: ProviderId::new("local").unwrap(),
                    locator: WorkspaceLocator::from_locator(&serde_json::json!({"path": "/repo"}))
                        .unwrap(),
                },
                id: RunId::new("remove-once").unwrap(),
            },
            "/checkout space\nline",
        )
        .unwrap()
    }

    #[test]
    fn defaults_and_serialization_preserve_removal_identity() {
        let request = request();
        assert_eq!(request.timeout_seconds, 120);
        assert_eq!(request.output_limit_bytes, 256 * 1024);
        let encoded = serde_json::to_value(&request).unwrap();
        let decoded: GitWorktreeRemoveRequest = serde_json::from_value(encoded.clone()).unwrap();
        decoded.validate().unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), encoded);
    }

    #[test]
    fn mutable_or_deserialized_requests_recheck_path_and_launch_bounds() {
        let mut request = request();
        for path in [
            "",
            "/",
            "relative",
            "//a",
            "/a/",
            "/a//b",
            "/a/./b",
            "/a/../b",
            "/a\0b",
            "/a\u{fffd}b",
        ] {
            request.path = path.into();
            assert!(request.validate().is_err(), "accepted {path:?}");
        }
        request.path = format!("/{}", "p".repeat(16 * 1024 - 1));
        request.validate().unwrap();
        request.path.push('é');
        assert!(request.validate().is_err());
        request.path = "/résumé space\nline".into();
        for (seconds, bytes) in [(1, 2), (3600, 16 * 1024 * 1024)] {
            request.timeout_seconds = seconds;
            request.output_limit_bytes = bytes;
            request.validate().unwrap();
        }
        for (seconds, bytes) in [(0, 2), (3601, 2), (1, 1), (1, 16 * 1024 * 1024 + 1)] {
            let mut encoded = serde_json::to_value(&request).unwrap();
            encoded["timeout_seconds"] = seconds.into();
            encoded["output_limit_bytes"] = bytes.into();
            let decoded: GitWorktreeRemoveRequest = serde_json::from_value(encoded).unwrap();
            assert!(decoded.validate().is_err());
        }
    }

    #[test]
    fn lexical_guards_protect_anchor_and_control_with_component_boundaries() {
        for target in ["/repo", "/repo/main", "/private", "/private/control"] {
            assert!(validate_anchor_paths(target, "/repo/main", "/private/control").is_err());
        }
        for target in [
            "/repo/linked",
            "/repo-main",
            "/private-other",
            "/private/control-sibling",
        ] {
            validate_anchor_paths(target, "/repo/main", "/private/control").unwrap();
        }
        validate_anchor_paths("/linked", "/", "/control").unwrap();
        for invalid in [
            "",
            "relative",
            "//a",
            "/a/",
            "/a/../b",
            "/a/./b",
            "/a//b",
            "/a\0b",
            "/a\u{fffd}b",
        ] {
            assert!(validate_anchor_paths("/linked", invalid, "/control").is_err());
            assert!(validate_anchor_paths("/linked", "/repo", invalid).is_err());
        }
        let oversized = format!("/{}", "a".repeat(16 * 1024));
        assert!(validate_anchor_paths("/linked", &oversized, "/control").is_err());
        assert!(validate_anchor_paths("/linked", "/repo", &oversized).is_err());
    }

    #[test]
    fn removal_is_literal_nonforce_argv_with_visible_untracked_guard() {
        let request = request();
        let command = remove_command("/opt/git with spaces", &request);
        assert_eq!(command.program, "/opt/git with spaces");
        assert_eq!(
            command.args,
            [
                "--no-pager",
                "-c",
                "status.showUntrackedFiles=all",
                "worktree",
                "remove",
                "--",
                &request.path
            ]
        );
        assert_eq!(command.timeout_seconds, Some(120));
        assert_eq!(command.cwd, crate::WorkspacePath::root());
        assert!(command.stdin.is_none());
        for key in [
            "GIT_TERMINAL_PROMPT",
            "GIT_CONFIG_COUNT",
            "GIT_OPTIONAL_LOCKS",
        ] {
            assert_eq!(command.env.get(key).map(String::as_str), Some("0"));
        }
        for key in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_CONFIG_PARAMETERS",
        ] {
            assert!(command.env_remove.iter().any(|name| name == key));
        }
        assert!(!command
            .env_remove
            .iter()
            .any(|key| key == "GIT_CONFIG_GLOBAL"));
        command.validate().unwrap();
    }
}
