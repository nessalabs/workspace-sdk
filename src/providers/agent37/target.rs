//! One attached Agent37 instance as a supervisor target: requests go
//! through the API's `exec`, and anything holding a credential through an
//! SSH connection's standard input instead, so it never reaches a command
//! line.
use super::{
    api::{Api, Refusal},
    identity::Agent37Identity,
    provider::Locator,
    tunnel::Tunnel,
};
use crate::{
    domain::*,
    ports::{IConnection, ProcessEnd, SocketForward},
    providers::{
        managed::{self, ITarget},
        openssh::{Route, SshConnection, Way},
    },
    supervisor::protocol::{SupervisorRequest, SupervisorValue},
    Error, Result,
};
use async_trait::async_trait;
use std::{io::Write, sync::Arc, time::Duration};

/// How long one request through a connection may take.
const PRIVATE_TIMEOUT: Duration = Duration::from_secs(120);

pub(super) struct Agent37Target {
    pub api: Api,
    pub identity: Arc<Agent37Identity>,
    pub user: &'static str,
    pub locator: Locator,
}

fn failure(refusal: Refusal) -> Error {
    Error::Provider {
        provider: "Agent37".into(),
        message: match refusal {
            Refusal::Answered { status, code } => format!("refused: {status} {code}"),
            Refusal::Unknown => "no answer: the request may have run".into(),
        },
    }
}

impl Agent37Target {
    async fn inline(&self, request: &SupervisorRequest) -> Result<SupervisorValue> {
        let text = serde_json::to_string(request)?;
        let ran = self
            .api
            .exec(
                &self.locator.instance,
                &format!(
                    "printf '%s' {} | {}",
                    managed::quote(&text),
                    managed::quote(&self.locator.supervisor)
                ),
            )
            .await
            .map_err(failure)?;
        if ran.truncated {
            return Err(Error::State(
                "Agent37 truncated a supervisor response; request a smaller page".into(),
            ));
        }
        managed::response(&ran.stdout, ran.exit_code == 0)
    }
    /// A request through a connection's standard input: nothing of it is
    /// in a command line, the API's records, or a file on the instance.
    async fn private(&self, request: &SupervisorRequest) -> Result<SupervisorValue> {
        let mut input = tempfile::tempfile()?;
        input.write_all(serde_json::to_string(request)?.as_bytes())?;
        std::io::Seek::rewind(&mut input)?;
        let connection = self.connect(None).await?;
        let ran = connection
            .run(
                &managed::quote(&self.locator.supervisor),
                Some(input),
                PRIVATE_TIMEOUT,
            )
            .await?;
        managed::response(&ran.output, ran.end == ProcessEnd::Exited(0))
    }
    async fn connect(&self, forward: Option<SocketForward>) -> Result<Arc<SshConnection>> {
        let config = self.api.config().clone();
        let identity = self.identity.clone();
        let instance = self.locator.instance.clone();
        let user = self.user;
        SshConnection::connect(
            |directory| async move {
                identity.write_private(&directory.join("key"))?;
                let socket = directory.join("tunnel.sock");
                let tunnel = Tunnel::serve(&config, &instance, &socket)?;
                Ok(Route {
                    user: user.into(),
                    way: Way::Tunnel {
                        socket,
                        host: format!("{instance}.agent37"),
                    },
                    keep: Some(Box::new(tunnel)),
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
        match self.status().await?.as_str() {
            // A sleeping instance wakes on the first request.
            "running" | "sleeping" => Ok(()),
            _ => Err(Error::State(
                "the Agent37 instance is not running; its owner must start it".into(),
            )),
        }
    }
    async fn state(&self) -> Result<WorkspaceState> {
        Ok(match self.status().await?.as_str() {
            "running" => WorkspaceState::Ready,
            "stopped" | "sleeping" | "stopping" => WorkspaceState::Stopped,
            "provisioning" | "starting" | "waking" | "restarting" | "updating" => {
                WorkspaceState::Provisioning
            }
            _ => WorkspaceState::Unknown,
        })
    }
    async fn connection(&self, forward: Option<SocketForward>) -> Result<Arc<dyn IConnection>> {
        Ok(self.connect(forward).await?)
    }
}
impl Agent37Target {
    async fn status(&self) -> Result<String> {
        match self.api.instance(&self.locator.instance).await {
            Ok(instance) => Ok(instance.status),
            Err(refusal) if refusal.status() == Some(404) => Ok("deleted".into()),
            Err(refusal) => Err(failure(refusal)),
        }
    }
}
