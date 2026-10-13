//! Real local workspaces and unconditional cleanup for PTY acceptance cases.
use std::{
    collections::BTreeMap,
    env, fs,
    future::Future,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tempfile::TempDir;
use tokio::time::{sleep, timeout};
use workspace_sdk::{
    providers::{LocalConfig, LocalProvider},
    runtime::*,
    BackendDescriptor, IProvider, OpenRequest, OutputCursor, SecretInput, WorkspaceId,
};

pub const INPUT_SECRET: &str = "pty-input-private-992c1\n";
pub const ENV_SECRET: &str = "pty-env-private-4f712";

fn program() -> &'static str {
    static FIXTURE: OnceLock<(TempDir, PathBuf)> = OnceLock::new();
    let (_, binary) = FIXTURE.get_or_init(|| {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("workspace-pty-fixture");
        let result = Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .args([
                "--edition=2021",
                concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/pty_workload.rs"),
                "-o",
            ])
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        (directory, binary)
    });
    binary.to_str().unwrap()
}

pub struct Fixture {
    pub directory: TempDir,
    pub descriptor: BackendDescriptor,
    provider: LocalProvider,
    processes: Mutex<Vec<RuntimeProcessRef>>,
}
impl Fixture {
    pub async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut config = LocalConfig::trusted_host(directory.path().join("workspaces"));
        config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
        let provider = LocalProvider::new(config).unwrap();
        let backend = provider
            .open(OpenRequest {
                workspace_id: WorkspaceId::generate(),
                config: Default::default(),
                resources: Default::default(),
            })
            .await
            .unwrap();
        Self {
            directory,
            descriptor: backend.descriptor(),
            provider,
            processes: Mutex::new(Vec::new()),
        }
    }
    pub async fn host(&self) -> BackendRuntimeHost {
        BackendRuntimeHost::new(
            self.provider
                .connect(&self.descriptor.reference)
                .await
                .unwrap(),
        )
    }
    pub fn command(&self, cwd: &str, args: &[&str]) -> RuntimeCommand {
        RuntimeCommand {
            program: program().into(),
            args: args.iter().map(|s| (*s).into()).collect(),
            working_directory: cwd.into(),
            environment: BTreeMap::new(),
            secret_environment: BTreeMap::new(),
            remove_environment: Vec::new(),
            timeout_seconds: 20,
        }
    }
    pub async fn start(
        &self,
        host: &dyn IRuntimeHost,
        key: &str,
        command: RuntimeCommand,
    ) -> RuntimeProcessRef {
        let process = host.start_interactive(key, command, true).await.unwrap();
        self.processes.lock().unwrap().push(process.clone());
        process
    }
    async fn cleanup(&self) -> Vec<String> {
        let backend = match self.provider.connect(&self.descriptor.reference).await {
            Ok(backend) => backend,
            Err(error) => return vec![format!("cleanup reconnect failed: {error}")],
        };
        let host = BackendRuntimeHost::new(backend);
        let processes = self.processes.lock().unwrap().clone();
        let mut failures = Vec::new();
        for process in processes {
            match timeout(Duration::from_secs(5), host.cancel_process(&process)).await {
                Ok(Ok(RuntimeProcessState::Exited { .. })) => {}
                other => {
                    failures.push(format!("cleanup could not confirm cancellation: {other:?}"))
                }
            }
        }
        if let Err(error) = host.backend().destroy().await {
            failures.push(format!("cleanup destroy failed: {error}"));
        }
        failures
    }
}

/// The outer task retains every workspace; even a case panic/timeout runs cleanup.
pub async fn run_case<F, Fut>(count: usize, case: F)
where
    F: FnOnce(Vec<Arc<Fixture>>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let mut fixtures = Vec::new();
    for _ in 0..count {
        fixtures.push(Arc::new(Fixture::new().await));
    }
    let mut task = tokio::spawn(case(fixtures.clone()));
    let outcome = match timeout(Duration::from_secs(40), &mut task).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(format!("PTY case failed: {error}")),
        Err(_) => {
            task.abort();
            let _ = task.await;
            Err("PTY case exceeded watchdog".into())
        }
    };
    let mut cleanup_failures = Vec::new();
    for fixture in fixtures {
        match timeout(Duration::from_secs(15), fixture.cleanup()).await {
            Ok(failures) => cleanup_failures.extend(failures),
            Err(_) => cleanup_failures.push("workspace cleanup exceeded watchdog".into()),
        }
    }
    assert!(
        cleanup_failures.is_empty(),
        "{cleanup_failures:?}; case={outcome:?}"
    );
    outcome.unwrap();
}

#[derive(Default)]
pub struct Transcript {
    pub text: String,
    pub cursor: OutputCursor,
}
impl Transcript {
    pub async fn until(
        &mut self,
        host: &dyn IRuntimeHost,
        process: &RuntimeProcessRef,
        marker: &str,
    ) {
        timeout(Duration::from_secs(10), async {
            while !self.text.contains(marker) {
                let page = host
                    .read_process_output(process, self.cursor.clone(), 4096)
                    .await
                    .unwrap();
                assert!(
                    !page.gap,
                    "small interactive transcript must not lose bytes"
                );
                assert!(page.stderr.is_empty());
                assert_eq!(page.next.stderr, 0);
                assert_eq!(
                    page.next.stdout,
                    self.cursor.stdout + page.stdout.len() as u64
                );
                self.text.push_str(&page.stdout);
                self.cursor = page.next;
                if !self.text.contains(marker) {
                    assert!(
                        !page.complete,
                        "terminal output ended before expected marker"
                    );
                    sleep(Duration::from_millis(10)).await;
                }
            }
        })
        .await
        .expect("interactive output deadline");
    }
    pub async fn complete(&mut self, host: &dyn IRuntimeHost, process: &RuntimeProcessRef) {
        timeout(Duration::from_secs(10), async {
            loop {
                let page = host
                    .read_process_output(process, self.cursor.clone(), 4096)
                    .await
                    .unwrap();
                assert!(!page.gap);
                assert!(page.stderr.is_empty());
                assert_eq!(page.next.stderr, 0);
                assert_eq!(
                    page.next.stdout,
                    self.cursor.stdout + page.stdout.len() as u64
                );
                self.text.push_str(&page.stdout);
                self.cursor = page.next;
                if page.complete {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("output must drain to confirmed EOF");
    }
}

pub async fn input(host: &dyn IRuntimeHost, process: &RuntimeProcessRef, bytes: &[u8]) {
    let private = SecretInput::new(bytes.to_vec());
    assert_eq!(
        host.write_process_input(process, &private).await.unwrap(),
        RuntimeInputOutcome::Accepted { bytes: bytes.len() }
    );
}

pub fn assert_no_secret_files(root: &Path) {
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        let metadata = fs::symlink_metadata(&path).unwrap();
        if metadata.is_dir() {
            assert_no_secret_files(&path);
        } else if metadata.is_file() {
            let content = fs::read(&path).unwrap();
            for secret in [INPUT_SECRET.trim_end().as_bytes(), ENV_SECRET.as_bytes()] {
                assert!(
                    !content.windows(secret.len()).any(|bytes| bytes == secret),
                    "transient credential persisted in {}",
                    path.display()
                );
            }
        }
    }
}
