//! Agent37's Sandbox API (`https://api.agent37.com/v1`): the few calls the
//! machines and the target make, each answer read strictly and bounded.
use super::{config::Agent37Config, failure::Agent37Failure};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};

/// The most of one answer that is read.
const MAX_ANSWER: usize = 4 * 1024 * 1024;
/// An `exec` is answered only once its command ends; a supervisor request
/// is short, so this bounds a machine that never answers.
const EXEC_TIMEOUT: Duration = Duration::from_secs(180);

/// Error codes that say the same request may succeed if sent again
/// shortly: the instance was mid-transition, or no host had room.
const TRANSIENT: [&str; 3] = ["try_again", "no_capacity", "capacity_unavailable"];
/// The longest error code kept; the API's own are short identifiers.
const MAX_CODE: usize = 64;

/// Where an instance stands, as the API reports it.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Status {
    Provisioning,
    Running,
    Stopping,
    Stopped,
    Starting,
    Restarting,
    Updating,
    Sleeping,
    Waking,
    Failed,
    Deleting,
    Deleted,
    /// A status this SDK does not know: treated as in transition.
    #[serde(other)]
    Unknown,
}
impl Status {
    /// Running nothing and billing its disk alone: stopped, or asleep. A
    /// sleeping instance wakes on its next request, so it counts as usable
    /// by its owner's attachment; a stopped one waits for `start`.
    pub fn at_rest(self) -> bool {
        matches!(self, Self::Stopped | Self::Sleeping)
    }
    /// Never to be used again.
    pub fn gone(self) -> bool {
        matches!(self, Self::Failed | Self::Deleting | Self::Deleted)
    }
}

/// One instance as the API describes it: only what this SDK reads.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(super) struct Instance {
    pub id: String,
    pub status: Status,
    pub template: String,
    #[serde(default)]
    pub template_revision: Option<u64>,
    #[serde(default)]
    pub image_digest: Option<String>,
    #[serde(default)]
    pub metadata: Option<BTreeMap<String, serde_json::Value>>,
}
impl Instance {
    /// A metadata value this SDK wrote, if it is a string.
    pub fn tag(&self, name: &str) -> Option<&str> {
        self.metadata.as_ref()?.get(name)?.as_str()
    }
}

#[derive(Deserialize)]
struct List<T> {
    data: Vec<T>,
}

/// What `POST /v1/instances` is sent.
#[derive(Serialize)]
pub(super) struct NewInstance<'a> {
    pub template: &'a str,
    pub name: &'a str,
    pub region: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<super::spec::Size>,
    pub metadata: BTreeMap<&'a str, &'a str>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: &'a BTreeMap<String, String>,
    pub auto_sleep: bool,
    pub idle_timeout_seconds: u64,
}

/// A command's result from `exec`.
#[derive(Clone, Debug, Deserialize)]
pub(super) struct Executed {
    pub exit_code: i32,
    pub stdout: String,
    #[serde(default)]
    pub truncated: bool,
}

/// What an API call that did not succeed tells: its status and code, or
/// that no answer was had at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Refusal {
    /// The API answered with this status and error code.
    Answered { status: u16, code: String },
    /// No answer, or one that could not be read: the request may have
    /// taken effect.
    Unknown,
}
impl Refusal {
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Answered { status, .. } => Some(*status),
            Self::Unknown => None,
        }
    }
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Answered { code, .. } => Some(code),
            Self::Unknown => None,
        }
    }
    /// Whether the same request may succeed if sent again shortly.
    pub fn transient(&self) -> bool {
        self.code().is_some_and(|code| TRANSIENT.contains(&code))
    }
}
impl From<Refusal> for Agent37Failure {
    fn from(refusal: Refusal) -> Self {
        match refusal {
            Refusal::Unknown => Self::Provider,
            Refusal::Answered { status, code } => match (status, code.as_str()) {
                (401 | 403, _) => Self::Unauthorized,
                (404, _) => Self::Missing,
                (402, _) | (_, "instance_limit_reached") => Self::Funds,
                (400, _) => Self::InvalidInput,
                _ => Self::Provider,
            },
        }
    }
}

#[derive(Deserialize)]
struct Envelope {
    error: ErrorBody,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum ErrorBody {
    Coded { code: String },
    Flat(String),
}

/// One workspace's Sandbox API.
#[derive(Clone)]
pub(super) struct Api {
    http: reqwest::Client,
    config: Agent37Config,
}
impl std::fmt::Debug for Api {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Api")
            .field("base", &self.config.api_base())
            .finish_non_exhaustive()
    }
}
impl Api {
    pub fn new(config: Agent37Config) -> Result<Self, Agent37Failure> {
        // Nothing else installs one for a library caller.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| Agent37Failure::Provider)?;
        Ok(Self { http, config })
    }
    pub fn config(&self) -> &Agent37Config {
        &self.config
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}/{path}", self.config.api_base()))
            .bearer_auth(self.config.key().expose())
    }

    async fn send<T: DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T, Refusal> {
        let mut response = request.send().await.map_err(|_| Refusal::Unknown)?;
        let status = response.status();
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Refusal::Unknown)? {
            if body.len() + chunk.len() > MAX_ANSWER {
                return Err(Refusal::Unknown);
            }
            body.extend_from_slice(&chunk);
        }
        if status.is_success() {
            return serde_json::from_slice(&body).map_err(|_| Refusal::Unknown);
        }
        // A gateway that answered without the API's envelope may not have
        // passed the request on, or may have: it is not known.
        let code = match serde_json::from_slice::<Envelope>(&body) {
            Ok(Envelope {
                error: ErrorBody::Coded { code } | ErrorBody::Flat(code),
            }) => code,
            Err(_) if status.is_server_error() => return Err(Refusal::Unknown),
            Err(_) => String::new(),
        };
        // Only a code the API defines for a request it did not act on
        // makes a server error certain.
        if status.is_server_error() && !TRANSIENT.contains(&code.as_str()) {
            return Err(Refusal::Unknown);
        }
        let code = if code.len() <= MAX_CODE
            && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            code
        } else {
            "unreadable".into()
        };
        Err(Refusal::Answered {
            status: status.as_u16(),
            code,
        })
    }

    pub async fn instances(&self) -> Result<Vec<Instance>, Refusal> {
        let list: List<Instance> = self
            .send(self.request(reqwest::Method::GET, "instances"))
            .await?;
        Ok(list.data)
    }
    pub async fn instance(&self, id: &str) -> Result<Instance, Refusal> {
        self.send(self.request(reqwest::Method::GET, &format!("instances/{id}")))
            .await
    }
    pub async fn create(&self, new: &NewInstance<'_>) -> Result<Instance, Refusal> {
        // Creation answers once the instance runs, which can take minutes.
        self.send(
            self.request(reqwest::Method::POST, "instances")
                .timeout(Duration::from_secs(300))
                .json(new),
        )
        .await
    }
    /// `start`, `stop` or `sleep`; the acknowledgement is not read.
    pub async fn lifecycle(&self, id: &str, action: &str) -> Result<(), Refusal> {
        let _: serde_json::Value = self
            .send(
                self.request(reqwest::Method::POST, &format!("instances/{id}/{action}"))
                    .timeout(Duration::from_secs(300)),
            )
            .await?;
        Ok(())
    }
    pub async fn delete(&self, id: &str) -> Result<(), Refusal> {
        let _: serde_json::Value = self
            .send(self.request(reqwest::Method::DELETE, &format!("instances/{id}")))
            .await?;
        Ok(())
    }
    /// Whether the template, or its pinned release, is published.
    pub async fn template(&self, name: &str) -> Result<(), Refusal> {
        let _: serde_json::Value = self
            .send(self.request(reqwest::Method::GET, &format!("templates/{name}")))
            .await?;
        Ok(())
    }
    /// Run `command` through `sh -c` as the image's user. A lost answer
    /// leaves it unknown whether the command ran; it is never sent again.
    pub async fn exec(&self, id: &str, command: &str) -> Result<Executed, Refusal> {
        self.send(
            self.request(reqwest::Method::POST, &format!("instances/{id}/exec"))
                .timeout(EXEC_TIMEOUT)
                .json(&serde_json::json!({ "command": command })),
        )
        .await
    }
    /// Register `public_key` for every instance of the workspace. One that
    /// is already registered is accepted as it is.
    pub async fn register_key(&self, public_key: &str, name: &str) -> Result<(), Refusal> {
        let sent: Result<serde_json::Value, _> = self
            .send(
                self.request(reqwest::Method::POST, "ssh-keys")
                    .json(&serde_json::json!({ "public_key": public_key, "name": name })),
            )
            .await;
        match sent {
            Ok(_) => Ok(()),
            Err(refusal) if refusal.code() == Some("ssh_key_exists") => Ok(()),
            Err(refusal) => Err(refusal),
        }
    }
}
