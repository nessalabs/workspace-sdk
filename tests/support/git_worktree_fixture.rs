//! Shared disposable Git/native fixtures and bounded ownership-aware cleanup.
//! Included directly by the create/remove integration executables.

use async_trait::async_trait;
use std::{
    fs,
    future::Future,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tempfile::TempDir;
use workspace_sdk::{
    git::GitWorktreeCreateRequest,
    infrastructure::FileStateRepository,
    providers::{LocalConfig, LocalOpenOptions, LocalProvider},
    *,
};

pub(super) fn git(root: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_str().is_some_and(|key| key.starts_with("GIT_")) {
            command.env_remove(key);
        }
    }
    let output = command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-C",
        ])
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "fixture Git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

type NativeClaims = Arc<Mutex<Vec<(Arc<dyn IBackend>, RunRef)>>>;

pub(super) struct Fixture {
    directory: Mutex<Option<TempDir>>,
    pub(super) root: PathBuf,
    pub(super) repository: PathBuf,
    pub(super) oid: String,
    pub(super) sdk: WorkspaceSdk,
    pub(super) lost_reply: Arc<AtomicBool>,
    pub(super) simulated_lost: Arc<AtomicBool>,
    claims: NativeClaims,
    uncertain: Arc<AtomicBool>,
    pub(super) descriptor_override: Arc<Mutex<Option<BackendDescriptor>>>,
}

impl Fixture {
    fn with_object_format(format: &str) -> Self {
        let temporary = TempDir::new().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let repository = root.join("anchor repository");
        fs::create_dir(&repository).unwrap();
        git(
            &repository,
            &[
                "init",
                "--quiet",
                "--initial-branch=main",
                &format!("--object-format={format}"),
            ],
        );
        git(
            &repository,
            &["config", "--local", "core.hooksPath", "/dev/null"],
        );
        git(
            &repository,
            &["config", "--local", "commit.gpgSign", "false"],
        );
        for (key, value) in [
            ("core.fsmonitor", "false"),
            ("core.untrackedCache", "false"),
            ("core.attributesFile", "/dev/null"),
            ("core.excludesFile", "/dev/null"),
            ("core.autocrlf", "false"),
        ] {
            git(&repository, &["config", "--local", key, value]);
        }
        fs::write(repository.join("shared.txt"), "pinned\n").unwrap();
        git(&repository, &["add", "shared.txt"]);
        git(&repository, &["commit", "--quiet", "-m", "pinned commit"]);
        let oid = git(&repository, &["rev-parse", "HEAD"]).trim().to_owned();
        let mut config = LocalConfig::trusted_host(root.join("owned"));
        config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
        let lost_reply = Arc::new(AtomicBool::new(false));
        let simulated_lost = Arc::new(AtomicBool::new(false));
        let claims = Arc::new(Mutex::new(Vec::new()));
        let uncertain = Arc::new(AtomicBool::new(false));
        let descriptor_override = Arc::new(Mutex::new(None));
        let sdk = WorkspaceSdk::builder(Arc::new(
            FileStateRepository::new(root.join("registry")).unwrap(),
        ))
        .provider(Arc::new(ReplyLossProvider {
            inner: LocalProvider::new(config).unwrap(),
            lose_next: lost_reply.clone(),
            simulated_lost: simulated_lost.clone(),
            claims: claims.clone(),
            uncertain: uncertain.clone(),
            descriptor_override: descriptor_override.clone(),
        }))
        .unwrap()
        .build()
        .unwrap();
        Self {
            directory: Mutex::new(Some(temporary)),
            root,
            repository,
            oid,
            sdk,
            lost_reply,
            simulated_lost,
            claims,
            uncertain,
            descriptor_override,
        }
    }

    fn preserve(&self) -> PathBuf {
        self.directory.lock().unwrap().take().unwrap().keep()
    }

    async fn cleanup(&self) -> Vec<String> {
        // Descriptor overrides exercise pure guards only; cleanup always uses
        // real native references and the original provider descriptor.
        *self.descriptor_override.lock().unwrap() = None;
        let claims = self.claims.lock().unwrap().clone();
        let mut failures = Vec::new();
        if self.uncertain.load(Ordering::SeqCst) {
            failures.push(
                "native start returned an unexpected identity; ownership is uncertain".into(),
            );
        }
        for (backend, run) in claims {
            let cleanup = async {
                backend.cancel(&run).await?;
                loop {
                    if backend.inspect_run(&run).await?.is_terminal() {
                        return Ok::<(), Error>(());
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            };
            match tokio::time::timeout(Duration::from_secs(5), cleanup).await {
                Ok(Ok(())) => {}
                other => failures.push(format!("native claim cleanup not confirmed: {other:?}")),
            }
        }
        failures
    }

    pub(super) async fn attach(&self, path: &Path, id: &str) -> Workspace {
        self.sdk
            .open(
                &ProviderId::new("local").unwrap(),
                OpenRequest::new(
                    WorkspaceId::new(id).unwrap(),
                    &LocalOpenOptions {
                        attach: Some(path.to_owned()),
                    },
                )
                .unwrap(),
            )
            .await
            .unwrap()
    }

    pub(super) fn request(
        &self,
        anchor: &Workspace,
        id: &str,
        branch: &str,
    ) -> GitWorktreeCreateRequest {
        GitWorktreeCreateRequest::new(
            RunRef {
                workspace: anchor.reference(),
                id: RunId::new(id).unwrap(),
            },
            self.root.join(branch).to_str().unwrap(),
            format!("refs/heads/{branch}"),
            &self.oid,
        )
        .unwrap()
    }

    pub(super) fn wrapper(&self, name: &str, hold: bool) -> (String, PathBuf) {
        self.wrapper_for(name, hold, "add")
    }

    pub(super) fn wrapper_for(&self, name: &str, hold: bool, operation: &str) -> (String, PathBuf) {
        assert!(matches!(operation, "add" | "remove"));
        let program = self.root.join(name);
        let counter = self.root.join(format!("{name}.invocations"));
        let real_git = Command::new("/bin/sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap();
        assert!(real_git.status.success());
        let real_git = String::from_utf8(real_git.stdout).unwrap();
        let body = if hold {
            "exec /bin/sleep 30".to_owned()
        } else {
            format!("exec {} \"$@\"", shell_quote(real_git.trim()))
        };
        fs::write(
            &program,
            format!(
                "#!/bin/sh\nprevious=\nfor argument\ndo\nif [ \"$previous\" = worktree ] && [ \"$argument\" = {} ]; then\nprintf 'one\\n' >> {}\n{body}\nfi\nprevious=$argument\ndone\nexec {} \"$@\"\n",
                shell_quote(operation), shell_quote(counter.to_str().unwrap()), shell_quote(real_git.trim())
            ),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        (program.to_str().unwrap().into(), counter)
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(super) async fn exited(run: &Run) -> RunState {
    run.wait(Duration::from_secs(15)).await.unwrap()
}

pub(super) async fn wait_for(path: &Path) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

pub(super) fn assert_once(counter: &Path) {
    assert_eq!(fs::read_to_string(counter).unwrap(), "one\n");
}

/// The outer task retains source/control storage and cleans actual native claims
/// even when the case panics or exceeds its watchdog. No numeric PID is signaled.
pub(super) async fn run_case<F, Fut>(case: F)
where
    F: FnOnce(Arc<Fixture>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    run_case_format("sha1", case).await;
}

pub(super) async fn run_case_format<F, Fut>(format: &str, case: F)
where
    F: FnOnce(Arc<Fixture>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let fixture = Arc::new(Fixture::with_object_format(format));
    let mut task = tokio::spawn(case(fixture.clone()));
    let outcome = match tokio::time::timeout(Duration::from_secs(40), &mut task).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(format!("Git worktree case failed: {error}")),
        Err(_) => {
            task.abort();
            let _ = task.await;
            Err("Git worktree case exceeded watchdog".into())
        }
    };
    let cleanup_fixture = fixture.clone();
    let mut cleanup = tokio::spawn(async move { cleanup_fixture.cleanup().await });
    let failures = match tokio::time::timeout(Duration::from_secs(15), &mut cleanup).await {
        Ok(Ok(failures)) => failures,
        Ok(Err(error)) => vec![format!("native cleanup task failed: {error}")],
        Err(_) => {
            cleanup.abort();
            let _ = cleanup.await;
            vec!["native cleanup exceeded watchdog".into()]
        }
    };
    if !failures.is_empty() {
        let retained = fixture.preserve();
        panic!(
            "{failures:?}; case={outcome:?}; retained fixture: {}",
            retained.display()
        );
    }
    outcome.unwrap();
}

// Forwarding seams lose one acknowledgement or simulate a Lost observation over
// a real native claim. Git execution and native persistence remain real.
struct ReplyLossProvider {
    inner: LocalProvider,
    lose_next: Arc<AtomicBool>,
    simulated_lost: Arc<AtomicBool>,
    claims: NativeClaims,
    uncertain: Arc<AtomicBool>,
    descriptor_override: Arc<Mutex<Option<BackendDescriptor>>>,
}
#[async_trait]
impl IProvider for ReplyLossProvider {
    fn id(&self) -> ProviderId {
        self.inner.id()
    }
    async fn open(&self, request: OpenRequest) -> Result<Arc<dyn IBackend>> {
        Ok(Arc::new(ReplyLossBackend {
            inner: self.inner.open(request).await?,
            lose_next: self.lose_next.clone(),
            simulated_lost: self.simulated_lost.clone(),
            claims: self.claims.clone(),
            uncertain: self.uncertain.clone(),
            descriptor_override: self.descriptor_override.clone(),
        }))
    }
    async fn connect(&self, reference: &WorkspaceRef) -> Result<Arc<dyn IBackend>> {
        Ok(Arc::new(ReplyLossBackend {
            inner: self.inner.connect(reference).await?,
            lose_next: self.lose_next.clone(),
            simulated_lost: self.simulated_lost.clone(),
            claims: self.claims.clone(),
            uncertain: self.uncertain.clone(),
            descriptor_override: self.descriptor_override.clone(),
        }))
    }
}
struct ReplyLossBackend {
    inner: Arc<dyn IBackend>,
    lose_next: Arc<AtomicBool>,
    simulated_lost: Arc<AtomicBool>,
    claims: NativeClaims,
    uncertain: Arc<AtomicBool>,
    descriptor_override: Arc<Mutex<Option<BackendDescriptor>>>,
}
#[async_trait]
impl IBackend for ReplyLossBackend {
    fn descriptor(&self) -> BackendDescriptor {
        self.descriptor_override
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| self.inner.descriptor())
    }
    async fn ensure_ready(&self) -> Result<()> {
        self.inner.ensure_ready().await
    }
    async fn refresh(&self) -> Result<WorkspaceState> {
        self.inner.refresh().await
    }
    async fn exec(&self, request: ExecutionRequest) -> Result<ExecResult> {
        self.inner.exec(request).await
    }
    async fn read_file(&self, request: ReadFile) -> Result<FileChunk> {
        self.inner.read_file(request).await
    }
    async fn write_file(&self, request: WriteFile) -> Result<()> {
        self.inner.write_file(request).await
    }
    async fn start(&self, request: LaunchRequest) -> Result<RunRef> {
        let intended = RunRef {
            workspace: self.inner.descriptor().reference,
            id: request.id.clone(),
        };
        {
            let mut claims = self.claims.lock().unwrap();
            if !claims
                .iter()
                .any(|(_, run)| run.workspace == intended.workspace && run.id == intended.id)
            {
                claims.push((self.inner.clone(), intended.clone()));
            }
        }
        let run = self.inner.start(request).await?;
        if run.workspace != intended.workspace || run.id != intended.id {
            // An unexpected target is not ours to cancel. Preserve control state
            // and report uncertainty instead of guessing cleanup authority.
            self.uncertain.store(true, Ordering::SeqCst);
            return Err(Error::State(
                "native start returned an unexpected identity".into(),
            ));
        }
        if self.lose_next.swap(false, Ordering::SeqCst) {
            return Err(Error::State("injected lost start reply".into()));
        }
        Ok(run)
    }
    async fn inspect_run(&self, run: &RunRef) -> Result<RunState> {
        let actual = self.inner.inspect_run(run).await?;
        if self.simulated_lost.load(Ordering::SeqCst) {
            Ok(RunState::Lost {
                reason: "injected uncertain observation over a native claim".into(),
            })
        } else {
            Ok(actual)
        }
    }
    async fn read_output(
        &self,
        run: &RunRef,
        cursor: OutputCursor,
        limit: u32,
    ) -> Result<OutputPage> {
        self.inner.read_output(run, cursor, limit).await
    }
    async fn cancel(&self, run: &RunRef) -> Result<RunState> {
        self.inner.cancel(run).await
    }
}
