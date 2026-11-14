//! One attached Agent37 instance as a supervisor target: requests go
//! through the API's `exec`, and anything holding a credential through an
//! SSH connection's standard input instead, so it never reaches a command
//! line.
use super::{
    api::{Api, Refusal, Status},
    identity::Agent37Identity,
    provider::Locator,
    tunnel::Tunnel,
};
use crate::{
    domain::*,
    ports::{IConnection, ProcessEnd, SocketForward},
    providers::{
        managed::{self, ITarget},
        openssh::{Dial, Route, SshConnection},
    },
    supervisor::protocol::{SupervisorRequest, SupervisorValue},
    Error, Result,
};
use async_trait::async_trait;
use std::{io::Write, sync::Arc, time::Duration};

/// How long one request through a connection may take.
const PRIVATE_TIMEOUT: Duration = Duration::from_secs(120);
/// How long waking a sleeping instance may take; Agent37's own edge gives
/// a wake three minutes.
const WAKE_TIMEOUT: Duration = Duration::from_secs(180);
/// How often a wake, or an `exec` refused while the instance was busy, is
/// tried again, and how many times a refused `exec` is.
const RETRY_PAUSE: Duration = Duration::from_secs(2);
const EXEC_TRIES: u32 = 4;

pub(super) struct Agent37Target {
    pub api: Api,
    pub identity: Arc<Agent37Identity>,
    pub user: &'static str,
    pub locator: Locator,
    /// The connection private requests share while its master lives.
    pub private_link: tokio::sync::Mutex<Option<Arc<SshConnection>>>,
}

fn failure(refusal: Refusal) -> Error {
    Error::Provider {
        provider: "Agent37".into(),
        message: match refusal {
            // The code is bounded and plain: the API reader keeps no other.
            Refusal::Answered { status, code } => format!("refused: {status} {code}"),
            Refusal::Unknown => "no answer: the request may have run".into(),
        },
    }
}

impl Agent37Target {
    /// The instance's status, or `None` when it is gone or not the
    /// workspace's.
    async fn status(&self) -> Result<Option<Status>> {
        match self.api.instance(&self.locator.instance).await {
            Ok(instance) => Ok(Some(instance.status)),
            Err(refusal) if refusal.status() == Some(404) => Ok(None),
            Err(refusal) => Err(failure(refusal)),
        }
    }
    async fn inline(&self, request: &SupervisorRequest) -> Result<SupervisorValue> {
        let text = serde_json::to_string(request)?;
        let command = format!(
            "printf '%s' {} | {}",
            managed::quote(&text),
            managed::quote(&self.locator.supervisor)
        );
        let mut tries = 0;
        let ran = loop {
            tries += 1;
            match self.api.exec(&self.locator.instance, &command).await {
                Ok(ran) => break ran,
                // The API says such a command never started: sending it
                // again cannot run it twice. Nothing else is sent again.
                Err(refusal) if refusal.transient() && tries < EXEC_TRIES => {
                    tokio::time::sleep(RETRY_PAUSE).await;
                }
                Err(refusal) => return Err(failure(refusal)),
            }
        };
        if ran.truncated {
            return Err(Error::State(
                "Agent37 truncated a supervisor response; request a smaller page".into(),
            ));
        }
        managed::response(&ran.stdout, ran.exit_code == 0)
    }
    /// A request through a connection's standard input: nothing of it is
    /// in a command line, the API's records, or a file on the instance.
    /// Requests share one connection while its master lives; each still
    /// signs in once over its own WebSocket, about a second.
    async fn private(&self, request: &SupervisorRequest) -> Result<SupervisorValue> {
        let mut input = tempfile::tempfile()?;
        input.write_all(serde_json::to_string(request)?.as_bytes())?;
        std::io::Seek::rewind(&mut input)?;
        let connection = {
            let mut link = self.private_link.lock().await;
            match link.as_ref().filter(|link| link.alive()) {
                Some(link) => link.clone(),
                None => {
                    let fresh = self.connect(None).await?;
                    *link = Some(fresh.clone());
                    fresh
                }
            }
        };
        let ran = connection
            .run(
                &managed::quote(&self.locator.supervisor),
                Some(input),
                PRIVATE_TIMEOUT,
            )
            .await?;
        if ran.end == ProcessEnd::Lost {
            // It may have run: the caller decides, and nothing is sent again.
            return Err(Error::Provider {
                provider: "Agent37".into(),
                message: "the connection was lost: the request may have run".into(),
            });
        }
        managed::response(&ran.output, ran.end == ProcessEnd::Exited(0))
    }
    /// Wake a sleeping instance before anything dials it: a wake through the
    /// tunnel would outlast an SSH login's wait.
    async fn wake(&self) -> Result<()> {
        let deadline = tokio::time::Instant::now() + WAKE_TIMEOUT;
        let mut asked = false;
        loop {
            match self.status().await? {
                Some(Status::Running) => return Ok(()),
                Some(Status::Sleeping) if !asked => {
                    match self.api.lifecycle(&self.locator.instance, "start").await {
                        Ok(()) => asked = true,
                        Err(refusal) if refusal.transient() => {}
                        Err(refusal) => return Err(failure(refusal)),
                    }
                }
                Some(Status::Sleeping | Status::Waking | Status::Starting) => {}
                _ => {
                    return Err(Error::State(
                        "the Agent37 instance is not running; its owner must start it".into(),
                    ))
                }
            }
            if tokio::time::Instant::now() + RETRY_PAUSE > deadline {
                return Err(Error::Timeout("the Agent37 instance did not wake".into()));
            }
            tokio::time::sleep(RETRY_PAUSE).await;
        }
    }
    pub(super) async fn connect(
        &self,
        forward: Option<SocketForward>,
    ) -> Result<Arc<SshConnection>> {
        let config = self.api.config().clone();
        let identity = self.identity.clone();
        let instance = self.locator.instance.clone();
        let user = self.user;
        SshConnection::connect(
            |directory| async move {
                let socket = directory.join("tunnel.sock");
                let tunnel = Tunnel::serve(&config, &instance, &socket)?;
                Ok(Route {
                    user: user.into(),
                    key: identity.pair().clone(),
                    dial: Dial::Tunnel {
                        socket,
                        host: format!("{instance}.agent37"),
                    },
                    guard: Some(Box::new(tunnel)),
                })
            },
            forward,
        )
        .await
    }
}

#[async_trait]
impl ITarget for Agent37Target {
    fn connects(&self) -> bool {
        true
    }
    async fn invoke(&self, request: SupervisorRequest) -> Result<SupervisorValue> {
        if request.operation.requires_private_transport() {
            self.private(&request).await
        } else {
            self.inline(&request).await
        }
    }
    async fn ensure(&self) -> Result<()> {
        // A sleeping instance is the owner's to use: it wakes on a request,
        // so waking it is not a lifecycle change. A stopped one waits for
        // its owner.
        self.wake().await
    }
    async fn state(&self) -> Result<WorkspaceState> {
        Ok(match self.status().await? {
            Some(Status::Running) => WorkspaceState::Ready,
            Some(status) if status.at_rest() => WorkspaceState::Stopped,
            Some(Status::Stopping) => WorkspaceState::Stopped,
            Some(
                Status::Provisioning
                | Status::Starting
                | Status::Waking
                | Status::Restarting
                | Status::Updating,
            ) => WorkspaceState::Provisioning,
            _ => WorkspaceState::Unknown,
        })
    }
    async fn connection(&self, forward: Option<SocketForward>) -> Result<Arc<dyn IConnection>> {
        self.wake().await?;
        Ok(self.connect(forward).await?)
    }
}
