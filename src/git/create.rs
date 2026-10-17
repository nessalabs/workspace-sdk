use super::{command::git_command, porcelain, GitWorktrees};
use crate::{CommandSpec, Error, ExecResult, LaunchRequest, Result, Run, RunRef};
use serde::{Deserialize, Serialize};

/// Caller-persisted inputs for one managed Git worktree creation attempt.
///
/// Persist this request before launch and preserve every field when retrying.
/// The existing native supervisor claims its run ID and complete launch command.
/// Incompatible reuse that reaches that claim conflicts; input validation or
/// commit verification can reject a changed request earlier.
/// Serialization is data, not authorization or proof of worktree ownership.
/// Deserialization does not validate: [`Self::validate`] is repeated at launch.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitWorktreeCreateRequest {
    /// Existing workspace-qualified run identity, chosen before first launch.
    pub run: RunRef,
    /// Absolute POSIX target-host path, at most 16 KiB, without empty, `.` or `..`
    /// components, NUL or replacement characters. This is a locator, not identity.
    pub path: String,
    /// New full local branch reference, at most 4 KiB, starting with `refs/heads/`.
    /// Git refuses an existing branch; creation never resets it.
    pub branch_reference: String,
    /// Full nonzero 40- or 64-hex object ID. Git resolves the object and checks its
    /// type during execution; structural validation does not prove it is a commit.
    pub commit_oid: String,
    /// Requested execution deadline, 1 through 3600 seconds; default 120.
    pub timeout_seconds: u64,
    /// Combined retained managed stdout/stderr budget, 2 bytes through 16 MiB;
    /// default 256 KiB. A bounded tail can have gaps, exposed by normal run output.
    pub output_limit_bytes: u64,
}

impl GitWorktreeCreateRequest {
    /// Construct and validate a request with a 120-second deadline and 256 KiB
    /// output retention. Executable availability and filesystem state are deferred.
    pub fn new(
        run: RunRef,
        path: impl Into<String>,
        branch_reference: impl Into<String>,
        commit_oid: impl Into<String>,
    ) -> Result<Self> {
        let request = Self {
            run,
            path: path.into(),
            branch_reference: branch_reference.into(),
            commit_oid: commit_oid.into(),
            timeout_seconds: 120,
            output_limit_bytes: 256 * 1024,
        };
        request.validate()?;
        Ok(request)
    }

    /// Check bounded locator, branch, object-ID shape, deadline and retention.
    /// This pure validation does not inspect a repository or authorize mutation.
    pub fn validate(&self) -> Result<()> {
        if self.path.len() > 16 * 1024
            || !self.path.starts_with('/')
            || self.path.contains(['\0', '\u{fffd}'])
            || self
                .path
                .split('/')
                .skip(1)
                .any(|part| matches!(part, "" | "." | ".."))
        {
            return Err(Error::InvalidInput(
                "invalid Git worktree creation path".into(),
            ));
        }
        let short_branch = self.branch_reference.strip_prefix("refs/heads/");
        if !porcelain::valid_branch(&self.branch_reference)
            || self.branch_reference.contains('\u{fffd}')
            || short_branch.is_none_or(|branch| branch.starts_with('-') || branch == "HEAD")
        {
            return Err(Error::InvalidInput(
                "invalid new Git branch reference".into(),
            ));
        }
        if !porcelain::valid_oid(&self.commit_oid)
            || self.commit_oid.bytes().all(|byte| byte == b'0')
        {
            return Err(Error::InvalidInput("invalid pinned Git object ID".into()));
        }
        if !(1..=3600).contains(&self.timeout_seconds)
            || !(2..=16 * 1024 * 1024).contains(&self.output_limit_bytes)
        {
            return Err(Error::InvalidInput(
                "Git worktree creation launch bounds exceeded".into(),
            ));
        }
        Ok(())
    }
}

impl GitWorktrees {
    /// Start or reconnect to one managed `git worktree add -b` execution.
    ///
    /// Requires a managed-run backend; built-in native backends retain the durable
    /// launch claim. Custom backends must honor [`Workspace::start_with`](crate::Workspace::start_with).
    /// The request must name this exact workspace. A ten-second Git verification
    /// requires the object to resolve to exactly the supplied full
    /// commit ID, rejecting abbreviations, refs and peeled tag-object IDs. Object
    /// replacements are disabled for verification and creation. No destination or
    /// branch-existence preflight blocks an identical retry after Git succeeds.
    ///
    /// Verification repeats before the creation claim on every call; it may fetch
    /// promised objects or write host-configured diagnostics. Only the managed
    /// creation command shares that claim. This point-in-time check does not
    /// atomically bind repository namespaces; the repository must remain stable.
    /// Retrying requires the repository and executable to remain available.
    /// Reconnect to an existing run without verification using
    /// `anchor.run(request.run.id.clone())` instead.
    ///
    /// A returned [`Run`] confirms launch handling, not Git success. Use its normal
    /// state, wait, output and cancellation methods. A wait timeout does not cancel;
    /// transport errors and `Lost` leave execution uncertain. Retry only the same
    /// persisted request and run identity. Dropping the future or handle does not
    /// cancel the target process, and this method never retries automatically.
    ///
    /// The caller must keep the repository stable and serialize access to an
    /// exclusive, quiescent destination, including against external creators. Git
    /// may use an existing empty directory. Its own failure/signal cleanup can
    /// remove an in-progress worktree; partial files or a new branch may also remain.
    /// This is not a transaction: the SDK issues no reset, force, rollback, cleanup,
    /// deletion or provider attachment. Trusted host Git hooks and filters still run
    /// with the target's authority and can change files or print secrets. Ordinary
    /// Git safeguards remain enabled. Inspect a fresh inventory after confirmed
    /// success; paths convey no ownership or protection from later edits.
    pub async fn start_create(&self, request: GitWorktreeCreateRequest) -> Result<Run> {
        request.validate()?;
        if request.run.workspace != self.anchor.reference() {
            return Err(Error::InvalidInput(
                "Git creation run belongs to a different workspace".into(),
            ));
        }
        let verification = self
            .anchor
            .exec(verification_command(
                &self.git_executable,
                &request.commit_oid,
            ))
            .await?;
        verify_commit_result(verification, &request.commit_oid)?;
        self.anchor
            .start_with(LaunchRequest {
                id: request.run.id.clone(),
                execution: create_command(&self.git_executable, &request).into(),
                output_limit_bytes: request.output_limit_bytes,
            })
            .await
    }
}

fn pinned_command(program: &str) -> CommandSpec {
    let mut command = git_command(program);
    command
        .env_remove
        .retain(|key| key != "GIT_NO_REPLACE_OBJECTS");
    command
        .env
        .insert("GIT_NO_REPLACE_OBJECTS".into(), "1".into());
    command
}

fn verification_command(program: &str, oid: &str) -> CommandSpec {
    pinned_command(program)
        .args([
            "--no-pager",
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{oid}^{{commit}}"),
        ])
        .timeout(10)
}

fn verify_commit_result(result: ExecResult, oid: &str) -> Result<()> {
    if !result.success()
        || result.output_truncated
        || result.stdout.len().saturating_add(result.stderr.len()) > porcelain::MAX_OUTPUT_BYTES
        || result.stdout.strip_suffix('\n') != Some(oid.to_ascii_lowercase().as_str())
    {
        return Err(Error::State(
            "Git worktree creation could not verify the exact pinned commit".into(),
        ));
    }
    Ok(())
}

fn create_command(program: &str, request: &GitWorktreeCreateRequest) -> CommandSpec {
    pinned_command(program)
        .args([
            "--no-pager",
            "worktree",
            "add",
            "--no-track",
            "-b",
            &request.branch_reference["refs/heads/".len()..],
            "--",
            &request.path,
            &request.commit_oid,
        ])
        .timeout(request.timeout_seconds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProviderId, RunId, WorkspaceId, WorkspaceLocator, WorkspaceRef};

    fn request() -> GitWorktreeCreateRequest {
        let run = RunRef {
            workspace: WorkspaceRef {
                version: 1,
                id: WorkspaceId::new("anchor").unwrap(),
                provider: ProviderId::new("local").unwrap(),
                locator: WorkspaceLocator::from_locator(&serde_json::json!({"path": "/repo"}))
                    .unwrap(),
            },
            id: RunId::new("create-once").unwrap(),
        };
        GitWorktreeCreateRequest::new(
            run,
            "/checkout space\nline",
            "refs/heads/new",
            "a".repeat(40),
        )
        .unwrap()
    }

    #[test]
    fn constructor_defaults_and_serialization_preserve_the_launch_request() {
        let request = request();
        assert_eq!(request.timeout_seconds, 120);
        assert_eq!(request.output_limit_bytes, 256 * 1024);
        let encoded = serde_json::to_value(&request).unwrap();
        let decoded: GitWorktreeCreateRequest = serde_json::from_value(encoded.clone()).unwrap();
        decoded.validate().unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), encoded);
    }

    #[test]
    fn paths_are_bounded_absolute_posix_locators_without_ambiguous_components() {
        let mut request = request();
        for path in [
            "",
            "/",
            "relative",
            "//server/path",
            "/a//b",
            "/a/",
            "/a/./b",
            "/a/../b",
            "/a\0b",
            "/a\u{fffd}b",
        ] {
            request.path = path.into();
            assert!(request.validate().is_err(), "accepted {path:?}");
        }
        for path in ["/a space\nand newline", "/a\\backslash:colon", "/résumé"] {
            request.path = path.into();
            request.validate().unwrap();
        }
        request.path = format!("/{}", "p".repeat(16 * 1024 - 1));
        request.validate().unwrap();
        request.path.push('é');
        assert!(request.validate().is_err());
    }

    #[test]
    fn branch_validation_preserves_git_guards_and_reference_bound() {
        let mut request = request();
        for reference in [
            "main",
            "refs/tags/new",
            "refs/heads/",
            "refs/heads/-flag",
            "refs/heads/HEAD",
            "refs/heads/a..b",
            "refs/heads/a.lock",
            "refs/heads/.hidden",
            "refs/heads/a@{b",
            "refs/heads/a b",
            "refs/heads/a\0b",
            "refs/heads/a\u{fffd}b",
        ] {
            request.branch_reference = reference.into();
            assert!(request.validate().is_err(), "accepted {reference:?}");
        }
        for reference in [
            "refs/heads/feature/new",
            "refs/heads/a/-part",
            "refs/heads/résumé",
            "refs/heads/@",
        ] {
            request.branch_reference = reference.into();
            request.validate().unwrap();
        }
        request.branch_reference = format!("refs/heads/{}", "b".repeat(4096 - "refs/heads/".len()));
        request.validate().unwrap();
        request.branch_reference.push('b');
        assert!(request.validate().is_err());
    }

    #[test]
    fn pinned_oid_requires_full_nonzero_hexadecimal_shape() {
        let mut request = request();
        for oid in [
            "HEAD".into(),
            "a".repeat(39),
            "a".repeat(41),
            "z".repeat(40),
            "0".repeat(40),
            "0".repeat(64),
        ] {
            request.commit_oid = oid;
            assert!(request.validate().is_err());
        }
        for oid in [
            "A".repeat(40),
            "b".repeat(64),
            format!("{}1", "0".repeat(39)),
        ] {
            request.commit_oid = oid;
            request.validate().unwrap();
        }
    }

    #[test]
    fn launch_bounds_are_rechecked_after_mutation_or_deserialization() {
        let mut request = request();
        for (seconds, bytes) in [(1, 2), (3600, 16 * 1024 * 1024)] {
            request.timeout_seconds = seconds;
            request.output_limit_bytes = bytes;
            request.validate().unwrap();
        }
        for (seconds, bytes) in [(0, 2), (3601, 2), (1, 1), (1, 16 * 1024 * 1024 + 1)] {
            let mut encoded = serde_json::to_value(&request).unwrap();
            encoded["timeout_seconds"] = seconds.into();
            encoded["output_limit_bytes"] = bytes.into();
            let decoded: GitWorktreeCreateRequest = serde_json::from_value(encoded).unwrap();
            assert!(decoded.validate().is_err());
        }
    }

    #[test]
    fn create_command_pins_argv_deadline_and_preserves_git_configuration() {
        let request = request();
        let command = create_command("/opt/git with spaces", &request);
        assert_eq!(command.program, "/opt/git with spaces");
        assert_eq!(
            command.args,
            [
                "--no-pager",
                "worktree",
                "add",
                "--no-track",
                "-b",
                "new",
                "--",
                &request.path,
                &request.commit_oid
            ]
        );
        assert_eq!(command.timeout_seconds, Some(request.timeout_seconds));
        assert_eq!(command.cwd, crate::WorkspacePath::root());
        assert!(command.stdin.is_none());
        assert_eq!(
            command.env.get("GIT_TERMINAL_PROMPT").map(String::as_str),
            Some("0")
        );
        assert_eq!(
            command.env.get("GIT_CONFIG_COUNT").map(String::as_str),
            Some("0")
        );
        assert!(command.env_remove.iter().any(|key| key == "GIT_DIR"));
        assert!(command
            .env_remove
            .iter()
            .any(|key| key == "GIT_CONFIG_PARAMETERS"));
        assert!(!command.env.contains_key("GIT_CONFIG_GLOBAL"));
        assert!(!command
            .env_remove
            .iter()
            .any(|key| key == "GIT_CONFIG_GLOBAL"));
        command.validate().unwrap();
    }

    #[test]
    fn commit_verification_queries_exact_commit_and_disables_object_replacement() {
        let request = request();
        let command = verification_command("/opt/git", &request.commit_oid);
        assert_eq!(
            command.args,
            [
                "--no-pager",
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{}^{{commit}}", request.commit_oid)
            ]
        );
        assert_eq!(command.timeout_seconds, Some(10));
        assert_eq!(
            command
                .env
                .get("GIT_NO_REPLACE_OBJECTS")
                .map(String::as_str),
            Some("1")
        );
        let create = create_command("/opt/git", &request);
        assert_eq!(
            create.env.get("GIT_NO_REPLACE_OBJECTS").map(String::as_str),
            Some("1")
        );
    }

    fn verification_result(oid: &str) -> ExecResult {
        ExecResult {
            exit_code: Some(0),
            stdout: format!("{oid}\n"),
            stderr: String::new(),
            timed_out: false,
            output_truncated: false,
        }
    }

    #[test]
    fn exact_commit_verification_refuses_ambiguous_or_incomplete_results_without_stderr() {
        let oid = "a".repeat(40);
        for (exit_code, timed_out, output_truncated) in [
            (None, false, false),
            (Some(1), false, false),
            (Some(0), true, false),
            (Some(0), false, true),
        ] {
            let mut result = verification_result(&oid);
            result.exit_code = exit_code;
            result.timed_out = timed_out;
            result.output_truncated = output_truncated;
            result.stderr = "private captured stderr".into();
            let error = verify_commit_result(result, &oid).unwrap_err();
            assert!(matches!(error, Error::State(_)));
            assert!(!error.to_string().contains("private captured stderr"));
        }
        for stdout in [
            oid.clone(),
            format!("{oid}\n{oid}\n"),
            format!(" {oid}\n"),
            format!("{oid}\r\n"),
            format!("{}\n", "b".repeat(40)),
            format!("{}\n", "a".repeat(64)),
        ] {
            let mut result = verification_result(&oid);
            result.stdout = stdout;
            assert!(verify_commit_result(result, &oid).is_err());
        }
        let mut result = verification_result(&oid);
        result.stderr = "x".repeat(porcelain::MAX_OUTPUT_BYTES);
        assert!(verify_commit_result(result, &oid).is_err());
    }

    #[test]
    fn exact_commit_verification_accepts_casefolded_request_and_complete_sha256() {
        for oid in ["a".repeat(40), "b".repeat(64)] {
            verify_commit_result(verification_result(&oid), &oid.to_uppercase()).unwrap();
        }
    }
}
