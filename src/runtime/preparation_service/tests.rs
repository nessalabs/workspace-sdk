use crate::runtime::*;
use crate::{Error, OutputCursor, OutputPage, Result, SecretInput};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::sleep;

#[path = "coordination_tests.rs"]
mod coordination_tests;
#[path = "interaction_tests.rs"]
mod interaction_tests;

#[derive(Default)]
struct HostState {
    installed: BTreeMap<String, String>,
    commands: Vec<RuntimeCommand>,
    directories: Vec<String>,
    installs: usize,
    fail_install_once: bool,
    fail_setup_probe: bool,
    setup_complete: bool,
    setup_calls: usize,
    authenticated: bool,
    native_api_only: bool,
    login_calls: usize,
    process_exited: bool,
    cache_writes: usize,
    logins: BTreeMap<String, RuntimeProcessRef>,
    inputs: Vec<Vec<u8>>,
    resizes: Vec<RuntimeTerminalSize>,
    input_outcome: Option<RuntimeInputOutcome>,
    oversized_output: bool,
    process_lost: bool,
    cancel_loses_response: bool,
    stall_input: bool,
}

#[derive(Default)]
struct FakeHost(Mutex<HostState>);

#[async_trait]
impl IRuntimeHost for FakeHost {
    fn scope_id(&self) -> Result<String> {
        Ok("fake-workspace-1".into())
    }
    fn validate_target(&self, target_id: &str) -> Result<()> {
        if target_id != "workspace-1" {
            return Err(Error::InvalidInput("wrong fake target".into()));
        }
        Ok(())
    }
    async fn inspect_resources(&self) -> Result<AvailableResources> {
        Ok(AvailableResources {
            os: "linux".into(),
            architecture: "x86_64".into(),
            cpus: Some(4),
            memory_mib: Some(8192),
            disk_mib: Some(50_000),
            gpus: Vec::new(),
        })
    }
    async fn ensure_private_directory(&self, path: &str) -> Result<()> {
        self.0.lock().unwrap().directories.push(path.into());
        Ok(())
    }
    async fn exec(&self, command: RuntimeCommand) -> Result<RuntimeOutput> {
        let mut state = self.0.lock().unwrap();
        state.commands.push(command.clone());
        let output = |exit_code, stdout: &str| RuntimeOutput {
            exit_code,
            stdout: stdout.into(),
            stderr: String::new(),
        };
        if command.args == ["--version"] {
            return Ok(state.installed.get(&command.program).map_or_else(
                || output(127, ""),
                |version| output(0, &format!("tool {version}")),
            ));
        }
        if command.program == "npm" {
            state.installs += 1;
            if state.fail_install_once {
                state.fail_install_once = false;
                return Ok(output(1, "sensitive installer output"));
            }
            let package = command.args.last().unwrap();
            let version = package.rsplit('@').next().unwrap();
            let program = if package.starts_with("@openai/") {
                "codex"
            } else if package.starts_with("opencode-ai") {
                "opencode"
            } else {
                "claude"
            };
            state.installed.insert(
                format!("{}/node_modules/.bin/{program}", command.args[2]),
                version.into(),
            );
            return Ok(output(0, "installed"));
        }
        if command.program == "check-deps" {
            if state.fail_setup_probe {
                return Err(Error::State("setup probe transport failed".into()));
            }
            return Ok(output(if state.setup_complete { 0 } else { 1 }, ""));
        }
        if command.program == "install-deps" {
            state.setup_calls += 1;
            state.setup_complete = true;
            return Ok(output(0, ""));
        }
        if command.args.last().is_some_and(|arg| arg == "--help") {
            return Ok(output(0, "--device-auth --provider --method"));
        }
        if command.args == ["login", "status"] {
            return Ok(if state.native_api_only {
                output(0, "Logged in using an API key")
            } else if state.authenticated {
                output(0, "Logged in using ChatGPT")
            } else {
                output(1, "Not logged in")
            });
        }
        if command.args == ["auth", "list"] {
            return Ok(if state.native_api_only {
                output(0, "OpenAI api")
            } else if state.authenticated {
                output(0, "OpenAI oauth")
            } else {
                output(0, "0 credentials")
            });
        }
        if command.args == ["auth", "status"] {
            return Ok(if state.native_api_only {
                output(0, r#"{"loggedIn":true,"authMethod":"api_key"}"#)
            } else if state.authenticated {
                output(0, r#"{"loggedIn":true,"authMethod":"claude.ai"}"#)
            } else {
                output(1, r#"{"loggedIn":false}"#)
            });
        }
        Ok(output(1, "unrecognized fake command"))
    }
    async fn start_interactive(
        &self,
        key: &str,
        command: RuntimeCommand,
        requires_terminal: bool,
    ) -> Result<RuntimeProcessRef> {
        let mut state = self.0.lock().unwrap();
        if let Some(process) = state.logins.get(key) {
            return Ok(process.clone());
        }
        assert_eq!(requires_terminal, command.args == ["auth", "login"]);
        state.commands.push(command);
        state.login_calls += 1;
        let process =
            RuntimeProcessRef::new(self.scope_id()?, format!("login-{}", state.login_calls))?;
        state.logins.insert(key.into(), process.clone());
        Ok(process)
    }
    async fn inspect_process(&self, _process: &RuntimeProcessRef) -> Result<RuntimeProcessState> {
        let state = self.0.lock().unwrap();
        Ok(if state.process_lost {
            RuntimeProcessState::Lost
        } else if state.process_exited {
            RuntimeProcessState::Exited { code: 1 }
        } else {
            RuntimeProcessState::Running
        })
    }

    async fn read_process_output(
        &self,
        _process: &RuntimeProcessRef,
        cursor: OutputCursor,
        limit: u32,
    ) -> Result<OutputPage> {
        let state = self.0.lock().unwrap();
        let stdout = if state.oversized_output {
            "x".repeat(limit as usize + 1)
        } else {
            "echoed-private-input"
                .chars()
                .take(limit as usize)
                .collect()
        };
        Ok(OutputPage {
            next: OutputCursor {
                stdout: cursor.stdout + stdout.len() as u64,
                stderr: cursor.stderr,
            },
            stdout,
            stderr: String::new(),
            gap: false,
            complete: state.process_exited,
        })
    }

    async fn write_process_input(
        &self,
        _process: &RuntimeProcessRef,
        input: &SecretInput,
    ) -> Result<RuntimeInputOutcome> {
        let (outcome, stall) = {
            let mut state = self.0.lock().unwrap();
            state.inputs.push(input.expose().to_vec());
            (
                state
                    .input_outcome
                    .unwrap_or(RuntimeInputOutcome::Accepted {
                        bytes: input.expose().len(),
                    }),
                state.stall_input,
            )
        };
        if stall {
            sleep(Duration::from_secs(10)).await;
        }
        Ok(outcome)
    }

    async fn resize_process(
        &self,
        _process: &RuntimeProcessRef,
        size: RuntimeTerminalSize,
    ) -> Result<()> {
        self.0.lock().unwrap().resizes.push(size);
        Ok(())
    }

    async fn cancel_process(&self, _process: &RuntimeProcessRef) -> Result<RuntimeProcessState> {
        let mut state = self.0.lock().unwrap();
        state.process_exited = true;
        if state.cancel_loses_response {
            return Err(Error::State("cancellation response lost".into()));
        }
        Ok(RuntimeProcessState::Exited { code: 130 })
    }
    async fn provision_private_file(&self, path: &str, value: &SecretValue) -> Result<()> {
        assert!(path.ends_with("/codex/auth.json"));
        assert!(value.expose().contains("tokens"));
        let mut state = self.0.lock().unwrap();
        state.cache_writes += 1;
        state.authenticated = true;
        Ok(())
    }
    async fn exec_with_secret_stdin(
        &self,
        command: RuntimeCommand,
        input: &SecretValue,
    ) -> Result<RuntimeOutput> {
        assert_eq!(command.args, ["login", "--with-api-key"]);
        assert!(!input.is_empty());
        let mut state = self.0.lock().unwrap();
        state.commands.push(command);
        state.native_api_only = true;
        Ok(RuntimeOutput::default())
    }
}

#[derive(Default)]
struct Credentials(Mutex<BTreeMap<String, CredentialResolution>>);

#[async_trait]
impl ICredentialResolver for Credentials {
    async fn resolve(&self, credential: &CredentialRef) -> Result<CredentialResolution> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(&credential.0)
            .cloned()
            .unwrap_or(CredentialResolution::Missing))
    }
}

fn request(recipe: RuntimeRecipe, authentication: Authentication) -> PreparationRequest {
    PreparationRequest {
        id: "prepare-1".into(),
        target_id: "workspace-1".into(),
        owned_root: "/private/sdk/runtime".into(),
        working_directory: "/workspace/project".into(),
        recipe,
        environment: EnvironmentSpec::default(),
        authentication,
    }
}

fn service() -> (
    PreparationService,
    Arc<FakeHost>,
    Arc<Credentials>,
    Arc<MemoryPreparationStore>,
) {
    let host = Arc::new(FakeHost::default());
    let credentials = Arc::new(Credentials::default());
    let store = Arc::new(MemoryPreparationStore::default());
    (
        PreparationService::new(host.clone(), credentials.clone(), store.clone()),
        host,
        credentials,
        store,
    )
}

fn ready(outcome: PreparationOutcome) -> PreparedRuntime {
    match outcome {
        PreparationOutcome::Ready(runtime) => runtime,
        other => panic!("expected Ready, got {other:?}"),
    }
}

#[tokio::test]
async fn installs_pinned_tool_once_and_rechecks_reality_on_resume() {
    let (service, host, credentials, store) = service();
    credentials.0.lock().unwrap().insert(
        "openai".into(),
        CredentialResolution::Available(SecretValue::new("key-fixture")),
    );
    let request = request(
        codex("0.101.0"),
        Authentication::ApiKey {
            credential: CredentialRef("openai".into()),
        },
    );
    let first = ready(service.prepare(&request).await.unwrap());
    assert!(first
        .launch_command(["exec", "hello"])
        .unwrap()
        .program
        .contains("/tools/codex/0.101.0/"));
    ready(service.prepare(&request).await.unwrap());
    assert_eq!(host.0.lock().unwrap().installs, 1);
    host.0.lock().unwrap().installed.clear();
    ready(service.prepare(&request).await.unwrap());
    assert_eq!(host.0.lock().unwrap().installs, 2);
    let receipt = store.load(&request.id).await.unwrap().unwrap();
    assert_eq!(receipt.stage, PreparationStage::Ready);
    assert!(!serde_json::to_string(&receipt)
        .unwrap()
        .contains("key-fixture"));
}

#[tokio::test]
async fn compatible_existing_tool_is_reused_without_replacement() {
    let (service, host, _, _) = service();
    host.0
        .lock()
        .unwrap()
        .installed
        .insert("trainer".into(), "1.3.0".into());
    let mut recipe = generic("trainer", "trainer", "1.2.0");
    recipe.compatible_versions = "^1.2".into();
    let runtime = ready(
        service
            .prepare(&request(recipe, Authentication::None))
            .await
            .unwrap(),
    );
    assert_eq!(
        runtime.launch_command(["train"]).unwrap().program,
        "trainer"
    );
    assert_eq!(
        runtime.receipt().installed_version.as_deref(),
        Some("1.3.0")
    );
    assert_eq!(host.0.lock().unwrap().installs, 0);
}

#[tokio::test]
async fn failed_install_resumes_and_setup_steps_verify_before_reuse() {
    let (service, host, _, _) = service();
    host.0.lock().unwrap().fail_install_once = true;
    let mut recipe = codex("0.101.0");
    recipe.setup.push(SetupStep {
        id: "dependencies".into(),
        probe: RecipeCommand::new("check-deps", Vec::<String>::new()),
        apply: RecipeCommand::new("install-deps", Vec::<String>::new()),
        secrets: Vec::new(),
    });
    let request = request(recipe, Authentication::Subscription);
    let failure = service.prepare(&request).await.unwrap();
    assert!(matches!(
        failure,
        PreparationOutcome::SetupFailed {
            stage: PreparationStage::Installing,
            retryable: true,
            ..
        }
    ));
    assert!(!format!("{failure:?}").contains("sensitive installer output"));
    assert!(matches!(
        service.prepare(&request).await.unwrap(),
        PreparationOutcome::NeedsAuthentication { .. }
    ));
    service.prepare(&request).await.unwrap();
    assert_eq!(host.0.lock().unwrap().setup_calls, 1);
    host.0.lock().unwrap().setup_complete = false;
    service.prepare(&request).await.unwrap();
    assert_eq!(host.0.lock().unwrap().setup_calls, 2);
}

#[tokio::test]
async fn setup_probe_transport_failure_never_applies_setup() {
    let (service, host, _, _) = service();
    {
        let mut state = host.0.lock().unwrap();
        state.installed.insert("trainer".into(), "1.0.0".into());
        state.fail_setup_probe = true;
    }
    let mut recipe = generic("trainer", "trainer", "1.0.0");
    recipe.setup.push(SetupStep {
        id: "dependencies".into(),
        probe: RecipeCommand::new("check-deps", Vec::<String>::new()),
        apply: RecipeCommand::new("install-deps", Vec::<String>::new()),
        secrets: Vec::new(),
    });

    let result = service
        .prepare(&request(recipe, Authentication::None))
        .await;

    assert!(matches!(result, Err(Error::State(_))));
    let state = host.0.lock().unwrap();
    assert_eq!(state.setup_calls, 0);
    assert!(state
        .commands
        .iter()
        .all(|command| command.program != "install-deps"));
}

#[tokio::test]
async fn missing_and_expired_keys_request_input_without_install_or_launch() {
    let (service, host, credentials, _) = service();
    let request = request(
        claude_code("2.1.260"),
        Authentication::ApiKey {
            credential: CredentialRef("anthropic".into()),
        },
    );
    let first = service.prepare(&request).await.unwrap();
    assert!(
        matches!(first, PreparationOutcome::NeedsInput { ref inputs, .. } if inputs[0].reason == InputReason::Missing)
    );
    credentials
        .0
        .lock()
        .unwrap()
        .insert("anthropic".into(), CredentialResolution::Expired);
    let expired = service.prepare(&request).await.unwrap();
    assert!(
        matches!(expired, PreparationOutcome::NeedsInput { ref inputs, .. } if inputs[0].reason == InputReason::Expired)
    );
    assert!(host.0.lock().unwrap().commands.is_empty());
}

#[tokio::test]
async fn native_login_reconnects_and_does_not_fall_back_to_api_billing() {
    let (service, host, credentials, _) = service();
    host.0.lock().unwrap().native_api_only = true;
    credentials.0.lock().unwrap().insert(
        "openai".into(),
        CredentialResolution::Available(SecretValue::new("ambient-key")),
    );
    let request = request(codex("0.101.0"), Authentication::Subscription);
    let PreparationOutcome::NeedsAuthentication { action, .. } =
        service.prepare(&request).await.unwrap()
    else {
        panic!("expected native login")
    };
    assert_eq!(action.kind, AuthenticationActionKind::DeviceLogin);
    let first = service
        .start_authentication(&request, &action)
        .await
        .unwrap();
    let second = service
        .start_authentication(&request, &action)
        .await
        .unwrap();
    assert_eq!(first.process, second.process);
    assert_eq!(host.0.lock().unwrap().login_calls, 1);
    {
        let mut state = host.0.lock().unwrap();
        state.native_api_only = false;
        state.authenticated = true;
    }
    let runtime = ready(service.prepare(&request).await.unwrap());
    assert_eq!(
        runtime.authentication(),
        &AuthenticationState::NativeSessionPresent
    );
    let command = runtime.launch_command(["exec", "hello"]).unwrap();
    assert!(command.secret_environment.is_empty());
    assert!(command
        .remove_environment
        .contains(&"OPENAI_API_KEY".into()));
    host.0.lock().unwrap().authenticated = false;
    assert!(matches!(
        service.check_authentication(&request).await.unwrap(),
        AuthenticationState::NeedsAuthentication(_)
    ));
}

#[tokio::test]
async fn claude_in_opencode_and_agent_sdk_reject_subscription_tokens() {
    let (service, host, _, _) = service();
    let anthropic = request(
        opencode("1.18.27", ModelProvider::Anthropic),
        Authentication::Subscription,
    );
    assert!(matches!(
        service.prepare(&anthropic).await,
        Err(Error::Unsupported(_))
    ));
    let mut sdk = generic("my-claude-agent", "my-agent", "1.0.0");
    sdk.runtime = RuntimeKind::ClaudeAgentSdk;
    assert!(matches!(
        service
            .prepare(&request(sdk, Authentication::Subscription))
            .await,
        Err(Error::Unsupported(_))
    ));
    let mut native = request(
        claude_code("2.1.260"),
        Authentication::ProviderCredentials {
            variables: BTreeMap::from([(
                "CLAUDE_CODE_OAUTH_TOKEN".into(),
                CredentialRef("token".into()),
            )]),
        },
    );
    assert!(matches!(
        service.prepare(&native).await,
        Err(Error::InvalidInput(_))
    ));
    native.authentication = Authentication::CodexAuthCache {
        credential: CredentialRef("token".into()),
    };
    assert!(matches!(
        service.prepare(&native).await,
        Err(Error::Unsupported(_))
    ));
    assert!(host.0.lock().unwrap().commands.is_empty());
}

#[tokio::test]
async fn unmodified_claude_uses_native_terminal_action_and_auth_json_status() {
    let (service, host, _, _) = service();
    let request = request(claude_code("2.1.260"), Authentication::Subscription);
    let PreparationOutcome::NeedsAuthentication { action, .. } =
        service.prepare(&request).await.unwrap()
    else {
        panic!("expected native login")
    };
    assert_eq!(
        action.kind,
        AuthenticationActionKind::NativeTerminalRequired
    );
    assert_eq!(
        service.login_command(&request).await.unwrap().args,
        ["auth", "login"]
    );
    service
        .start_authentication(&request, &action)
        .await
        .unwrap();
    host.0.lock().unwrap().authenticated = true;
    ready(service.prepare(&request).await.unwrap());
    assert!(host
        .0
        .lock()
        .unwrap()
        .commands
        .iter()
        .all(|command| !command.args.contains(&"setup-token".into())));
}

#[tokio::test]
async fn opencode_has_own_headless_flow_and_server_requires_password() {
    let (service, host, _, _) = service();
    let request = request(
        opencode("1.18.27", ModelProvider::OpenAi),
        Authentication::Subscription,
    );
    service.prepare(&request).await.unwrap();
    let login = service.login_command(&request).await.unwrap();
    assert_eq!(
        login.args,
        [
            "auth",
            "login",
            "--provider",
            "openai",
            "--method",
            "ChatGPT Pro/Plus (headless)"
        ]
    );
    host.0.lock().unwrap().authenticated = true;
    let runtime = ready(service.prepare(&request).await.unwrap());
    assert!(runtime.opencode_server(4096, SecretValue::new("")).is_err());
    let server = runtime
        .opencode_server(4096, SecretValue::new("server-fixture"))
        .unwrap();
    assert_eq!(
        server.command.args,
        ["serve", "--hostname", "127.0.0.1", "--port", "4096"]
    );
    assert!(server.requires_authenticated_endpoint);
    assert_eq!(server.health_path, "/global/health");
    assert!(!format!("{server:?}").contains("server-fixture"));
}

#[tokio::test]
async fn codex_cache_requires_explicit_action_and_is_not_continuously_mirrored() {
    let (service, host, credentials, store) = service();
    credentials.0.lock().unwrap().insert(
        "cache".into(),
        CredentialResolution::Available(SecretValue::new(
            r#"{"tokens":{"access_token":"cache-fixture"}}"#,
        )),
    );
    let request = request(
        codex("0.101.0"),
        Authentication::CodexAuthCache {
            credential: CredentialRef("cache".into()),
        },
    );
    let PreparationOutcome::NeedsAuthentication { action, .. } =
        service.prepare(&request).await.unwrap()
    else {
        panic!("expected explicit cache action")
    };
    assert_eq!(action.kind, AuthenticationActionKind::ProvisionCodexCache);
    assert_eq!(host.0.lock().unwrap().cache_writes, 0);
    service
        .start_authentication(&request, &action)
        .await
        .unwrap();
    credentials.0.lock().unwrap().clear();
    ready(service.prepare(&request).await.unwrap());
    ready(service.prepare(&request).await.unwrap());
    assert_eq!(host.0.lock().unwrap().cache_writes, 1);
    assert!(
        !serde_json::to_string(&store.load(&request.id).await.unwrap())
            .unwrap()
            .contains("cache-fixture")
    );
}

#[tokio::test]
async fn version_resource_and_private_directory_constraints_fail_before_commands() {
    let (service, host, _, _) = service();
    let invalid_version = request(codex("latest"), Authentication::Subscription);
    assert!(matches!(
        service.prepare(&invalid_version).await,
        Err(Error::InvalidInput(_))
    ));
    let mut gpu = request(generic("trainer", "trainer", "1.0.0"), Authentication::None);
    gpu.environment.resources.gpu = Some(GpuRequirement {
        count: 1,
        min_memory_mib: Some(24000),
        model: None,
    });
    assert!(matches!(
        service.prepare(&gpu).await,
        Err(Error::Unsupported(_))
    ));
    gpu.environment.resources = ResourceRequirements::default();
    gpu.owned_root = "/workspace/project/private".into();
    assert!(matches!(
        service.prepare(&gpu).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(host.0.lock().unwrap().commands.is_empty());
}

#[tokio::test]
async fn generic_training_secrets_are_scoped_and_setup_receipts_are_nonsecret() {
    let (service, host, credentials, store) = service();
    host.0
        .lock()
        .unwrap()
        .installed
        .insert("trainer".into(), "1.0.0".into());
    credentials.0.lock().unwrap().insert(
        "dataset".into(),
        CredentialResolution::Available(SecretValue::new("dataset-fixture")),
    );
    credentials.0.lock().unwrap().insert(
        "registry".into(),
        CredentialResolution::Available(SecretValue::new("registry-fixture")),
    );
    let mut request = request(generic("trainer", "trainer", "1.0.0"), Authentication::None);
    request.environment.secrets.push(SecretRequirement {
        variable: "DATASET_TOKEN".into(),
        credential: CredentialRef("dataset".into()),
        description: "Training dataset access".into(),
    });
    request.recipe.setup.push(SetupStep {
        id: "dependencies".into(),
        probe: RecipeCommand::new("check-deps", Vec::<String>::new()),
        apply: RecipeCommand::new("install-deps", Vec::<String>::new()),
        secrets: vec![SecretRequirement {
            variable: "REGISTRY_TOKEN".into(),
            credential: CredentialRef("registry".into()),
            description: "Private package registry".into(),
        }],
    });
    let runtime = ready(service.prepare(&request).await.unwrap());
    let launch = runtime.launch_command(["--dataset", "data.json"]).unwrap();
    assert_eq!(
        launch.secret_environment["DATASET_TOKEN"].expose(),
        "dataset-fixture"
    );
    assert!(!launch.secret_environment.contains_key("REGISTRY_TOKEN"));
    {
        let state = host.0.lock().unwrap();
        let setup = state
            .commands
            .iter()
            .find(|command| command.program == "install-deps")
            .unwrap();
        assert!(setup.secret_environment.contains_key("REGISTRY_TOKEN"));
        assert!(!setup.secret_environment.contains_key("DATASET_TOKEN"));
    }
    let persisted = serde_json::to_string(&store.load(&request.id).await.unwrap()).unwrap();
    assert!(!persisted.contains("fixture"));
    assert!(!format!("{runtime:?}").contains("fixture"));
}

#[tokio::test]
async fn preparation_id_conflicts_and_file_store_survives_restart() {
    let (service, _, _, memory) = service();
    let request = request(codex("0.101.0"), Authentication::Subscription);
    service.prepare(&request).await.unwrap();
    let mut changed = request.clone();
    changed.recipe.version = "0.102.0".into();
    changed.recipe.compatible_versions = "=0.102.0".into();
    assert!(matches!(
        service.prepare(&changed).await,
        Err(Error::Conflict(_))
    ));
    let root = tempfile::tempdir().unwrap();
    let receipt = memory.load(&request.id).await.unwrap().unwrap();
    FilePreparationStore::new(root.path())
        .unwrap()
        .save(&receipt)
        .await
        .unwrap();
    let reloaded = FilePreparationStore::new(root.path())
        .unwrap()
        .load(&request.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt, reloaded);
    let mut conflict = receipt;
    conflict.request_fingerprint = "different".into();
    assert!(matches!(
        FilePreparationStore::new(root.path())
            .unwrap()
            .save(&conflict)
            .await,
        Err(Error::Conflict(_))
    ));
}

#[tokio::test]
async fn subscription_and_api_billing_use_separate_native_auth_directories() {
    let (service, host, credentials, _) = service();
    host.0.lock().unwrap().authenticated = true;
    let mut subscription = request(
        opencode("1.18.27", ModelProvider::OpenAi),
        Authentication::Subscription,
    );
    let subscribed = ready(service.prepare(&subscription).await.unwrap())
        .launch_command(["run", "hello"])
        .unwrap();
    credentials.0.lock().unwrap().insert(
        "key".into(),
        CredentialResolution::Available(SecretValue::new("api-fixture")),
    );
    subscription.id = "prepare-api".into();
    subscription.authentication = Authentication::ApiKey {
        credential: CredentialRef("key".into()),
    };
    let api = ready(service.prepare(&subscription).await.unwrap())
        .launch_command(["run", "hello"])
        .unwrap();
    assert_ne!(
        subscribed.environment["XDG_DATA_HOME"],
        api.environment["XDG_DATA_HOME"]
    );
    assert!(subscribed.secret_environment.is_empty());
    assert_eq!(
        api.secret_environment["OPENAI_API_KEY"].expose(),
        "api-fixture"
    );
}
