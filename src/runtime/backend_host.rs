use super::*;
use crate::domain::{
    CommandSpec, ExecResult, ExecutionRequest, LaunchRequest, OutputCursor, OutputPage, RunId,
    RunRef, RunState, SecretEnvironment, SecretInput, WorkspacePath,
};
use crate::{Error, IBackend, Result};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};
use std::sync::Arc;

/// Runtime preparation through the workspace backend's typed native operations.
///
/// The backend's Rust supervisor enforces workspace/control directory boundaries,
/// private file permissions, argument vectors, and transient secret transport.
/// This adapter performs no shell interpolation or embedded-script execution.
pub struct BackendRuntimeHost {
    backend: Arc<dyn IBackend>,
}

/// An adapter-local execution plan preserves the distinction between project and
/// private working directories until the matching backend operation is invoked.
struct BackendExecution {
    request: ExecutionRequest,
    private_cwd: Option<WorkspacePath>,
}

impl BackendRuntimeHost {
    /// Bind runtime preparation to one connected workspace backend.
    pub fn new(backend: Arc<dyn IBackend>) -> Self {
        Self { backend }
    }

    /// Borrow the connected backend for explicit lifecycle or output operations.
    pub fn backend(&self) -> &Arc<dyn IBackend> {
        &self.backend
    }

    /// Adapt a project-directory command for the backend's ordinary execution API.
    ///
    /// Private control-directory commands require a distinct backend operation;
    /// use [`Self::launch`] or [`IRuntimeHost::exec`] to preserve that scope.
    pub fn execution_request(&self, command: RuntimeCommand) -> Result<ExecutionRequest> {
        let execution = self.adapt_execution(command)?;
        if execution.private_cwd.is_some() {
            return Err(Error::InvalidInput(
                "private runtime commands must use the host exec or launch operation".into(),
            ));
        }
        Ok(execution.request)
    }

    fn adapt_execution(&self, command: RuntimeCommand) -> Result<BackendExecution> {
        let descriptor = self.backend.descriptor();
        validate_path(&descriptor.root)?;
        validate_path(&descriptor.control_root)?;
        validate_path(&command.working_directory)?;
        let (cwd, private_cwd) = if within(&command.working_directory, &descriptor.control_root) {
            (
                WorkspacePath::root(),
                Some(relative_path(
                    &command.working_directory,
                    &descriptor.control_root,
                )?),
            )
        } else if within(&command.working_directory, &descriptor.root) {
            (
                relative_path(&command.working_directory, &descriptor.root)?,
                None,
            )
        } else {
            return Err(Error::InvalidInput(
                "runtime working directory must remain in this workspace or its private control directory".into(),
            ));
        };
        let mut spec = CommandSpec::new(command.program).args(command.args);
        spec.cwd = cwd;
        spec.env = command.environment;
        spec.env_remove = command.remove_environment;
        spec.timeout_seconds = (command.timeout_seconds > 0).then_some(command.timeout_seconds);
        spec.validate()?;
        let secrets = command
            .secret_environment
            .into_iter()
            .map(|(key, value)| (key, value.expose().to_owned()))
            .collect();
        Ok(BackendExecution {
            request: ExecutionRequest {
                command: spec,
                secrets: SecretEnvironment(secrets),
                private_stdin: None,
            },
            private_cwd,
        })
    }

    /// Start or reconnect to a managed workload without losing its cwd scope.
    pub async fn launch(
        &self,
        id: RunId,
        command: RuntimeCommand,
        output_limit_bytes: u64,
    ) -> Result<RunRef> {
        let execution = self.adapt_execution(command)?;
        let request = LaunchRequest {
            id,
            execution: execution.request,
            output_limit_bytes,
        };
        match execution.private_cwd {
            Some(cwd) => self.backend.start_private(request, cwd).await,
            None => self.backend.start(request).await,
        }
    }

    /// Read native login interaction output through the managed run API.
    /// Device flows print their browser URL and one-time code here. The SDK never
    /// parses or stores subscription tokens as part of this operation.
    pub async fn read_authentication_output(
        &self,
        process: &RuntimeProcessRef,
        cursor: OutputCursor,
        limit: u32,
    ) -> Result<OutputPage> {
        self.backend
            .read_output(&self.run_ref(process)?, cursor, limit)
            .await
    }

    fn run_ref(&self, process: &RuntimeProcessRef) -> Result<RunRef> {
        if process.host_scope() != self.process_scope()? {
            return Err(Error::InvalidInput(
                "native process belongs to another host scope".into(),
            ));
        }
        Ok(RunRef {
            workspace: self.backend.descriptor().reference,
            id: RunId::new(process.id())?,
        })
    }

    fn process_scope(&self) -> Result<String> {
        Ok(format!(
            "workspace-v1-{:x}",
            Sha256::digest(serde_json::to_vec(&self.backend.descriptor().reference)?)
        ))
    }

    fn private_path(&self, path: &str) -> Result<WorkspacePath> {
        validate_path(path)?;
        let root = self.backend.descriptor().control_root;
        validate_path(&root)?;
        if path == root || !within(path, &root) {
            return Err(Error::InvalidInput(
                "runtime private path must be beneath this workspace's control directory".into(),
            ));
        }
        relative_path(path, &root)
    }

    async fn execute(&self, execution: BackendExecution) -> Result<RuntimeOutput> {
        let output = match execution.private_cwd {
            Some(cwd) => self.backend.exec_private(execution.request, cwd).await?,
            None => self.backend.exec(execution.request).await?,
        };
        runtime_output(output)
    }
}

#[async_trait]
impl IRuntimeHost for BackendRuntimeHost {
    fn scope_id(&self) -> Result<String> {
        self.process_scope()
    }
    fn validate_target(&self, target_id: &str) -> Result<()> {
        if self.backend.descriptor().reference.id.as_str() != target_id {
            return Err(Error::InvalidInput(
                "preparation belongs to another workspace".into(),
            ));
        }
        Ok(())
    }
    async fn inspect_resources(&self) -> Result<AvailableResources> {
        let observed = self.backend.inspect_resources().await?;
        Ok(AvailableResources {
            os: observed.os,
            architecture: observed.architecture,
            cpus: observed.cpus,
            memory_mib: observed.memory_mib,
            disk_mib: observed.disk_mib,
            gpus: observed
                .gpus
                .into_iter()
                .map(|gpu| AvailableGpu {
                    model: gpu.model,
                    memory_mib: gpu.memory_mib,
                })
                .collect(),
        })
    }

    async fn exec(&self, command: RuntimeCommand) -> Result<RuntimeOutput> {
        self.execute(self.adapt_execution(command)?).await
    }

    async fn exec_with_secret_stdin(
        &self,
        command: RuntimeCommand,
        input: &SecretValue,
    ) -> Result<RuntimeOutput> {
        if input.expose().len() > 65_536 || input.expose().contains('\0') {
            return Err(Error::InvalidInput(
                "private native login input must be at most 64 KiB without NUL".into(),
            ));
        }
        let mut execution = self.adapt_execution(command)?;
        execution.request.private_stdin =
            Some(SecretInput::new(input.expose().as_bytes().to_vec()));
        self.execute(execution).await
    }

    async fn ensure_private_directory(&self, path: &str) -> Result<()> {
        self.backend
            .ensure_private_directory(self.private_path(path)?)
            .await
    }

    async fn provision_private_file(&self, path: &str, value: &SecretValue) -> Result<()> {
        if value.expose().len() > 65_536 || value.expose().contains('\0') {
            return Err(Error::InvalidInput(
                "private runtime file exceeds 64 KiB or contains NUL".into(),
            ));
        }
        self.backend
            .write_private_file(self.private_path(path)?, value.expose().as_bytes())
            .await
    }

    async fn start_interactive(
        &self,
        idempotency_key: &str,
        command: RuntimeCommand,
        requires_terminal: bool,
    ) -> Result<RuntimeProcessRef> {
        if requires_terminal {
            return Err(Error::Unsupported("this backend has no interactive terminal; run the supplied native Claude login command in a target terminal, then check authentication again".into()));
        }
        let id = RunId::new(format!(
            "runtime-auth-{:x}",
            Sha256::digest(idempotency_key.as_bytes())
        ))?;
        let run = self.launch(id, command, 128 * 1024).await?;
        RuntimeProcessRef::new(self.process_scope()?, run.id.to_string())
    }

    async fn inspect_process(&self, process: &RuntimeProcessRef) -> Result<RuntimeProcessState> {
        Ok(process_state(
            self.backend.inspect_run(&self.run_ref(process)?).await?,
        ))
    }

    async fn read_process_output(
        &self,
        process: &RuntimeProcessRef,
        cursor: OutputCursor,
        limit: u32,
    ) -> Result<OutputPage> {
        self.read_authentication_output(process, cursor, limit)
            .await
    }

    async fn cancel_process(&self, process: &RuntimeProcessRef) -> Result<RuntimeProcessState> {
        Ok(process_state(
            self.backend.cancel(&self.run_ref(process)?).await?,
        ))
    }
}

fn process_state(state: RunState) -> RuntimeProcessState {
    match state {
        RunState::Starting | RunState::Running => RuntimeProcessState::Running,
        RunState::Exited { exit_code } => RuntimeProcessState::Exited { code: exit_code },
        RunState::Canceled => RuntimeProcessState::Exited { code: 130 },
        RunState::TimedOut => RuntimeProcessState::Exited { code: 124 },
        RunState::Lost { .. } => RuntimeProcessState::Lost,
    }
}

fn runtime_output(output: ExecResult) -> Result<RuntimeOutput> {
    if output.output_truncated {
        return Err(Error::State(
            "runtime command output was truncated; preparation cannot verify it reliably".into(),
        ));
    }
    if output.timed_out {
        return Err(Error::Timeout(
            "runtime command exceeded its preparation timeout".into(),
        ));
    }
    let exit_code = output
        .exit_code
        .ok_or_else(|| Error::State("runtime command ended without an exit status".into()))?;
    Ok(RuntimeOutput {
        exit_code,
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

fn relative_path(path: &str, root: &str) -> Result<WorkspacePath> {
    let relative = Path::new(path)
        .strip_prefix(root)
        .map_err(|_| Error::InvalidInput("runtime path is outside its selected root".into()))?;
    WorkspacePath::new(if relative.as_os_str().is_empty() {
        ".".to_owned()
    } else {
        relative.to_string_lossy().into_owned()
    })
}

fn within(path: &str, root: &str) -> bool {
    path == root || path.starts_with(&format!("{}/", root.trim_end_matches('/')))
}

fn validate_path(value: &str) -> Result<()> {
    let path = Path::new(value);
    if !path.is_absolute()
        || value.contains(['\0', '\n', '\\'])
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(Error::InvalidInput(
            "expected a normalized absolute POSIX runtime path".into(),
        ));
    }
    Ok(())
}
