use crate::{Error, Result, SecretInput};
use reqwest::{redirect::Policy, Client, Url};
use serde::{Deserialize, Serialize};
use std::{fmt, net::IpAddr, time::Duration};

/// Explicit credentials and endpoints for an existing SPACE installation.
/// Neither management nor attach tokens enter serialized workspace references.
/// The same explicit guest user runs the supervisor and writes its private files.
#[derive(Clone)]
pub struct SpaceConfig {
    pub(super) api: Url,
    pub(super) data: Url,
    pub(super) token: SecretInput,
    pub(super) user: String,
    pub(super) supervisor: String,
    pub(super) timeout: Duration,
    pub(super) client: Client,
}
impl fmt::Debug for SpaceConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpaceConfig")
            .field("api", &self.api)
            .field("data", &self.data)
            .field("user", &self.user)
            .field("supervisor", &self.supervisor)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}
impl SpaceConfig {
    /// Endpoints must be HTTPS origins, except literal loopback HTTP fixtures.
    /// No environment token discovery, proxy inheritance, redirects or implicit
    /// supervisor upload/provisioning occurs. The executable must be installed.
    pub fn new(
        api_url: impl Into<String>,
        data_plane_url: impl Into<String>,
        token: SecretInput,
        user: impl Into<String>,
        supervisor: impl Into<String>,
    ) -> Result<Self> {
        let api = origin(&api_url.into())?;
        let data = origin(&data_plane_url.into())?;
        let user = user.into();
        let supervisor = supervisor.into();
        if user.is_empty()
            || user.len() > 64
            || !user
                .bytes()
                .all(|v| v.is_ascii_alphanumeric() || b"_-".contains(&v))
            || user.starts_with('-')
        {
            return Err(Error::InvalidInput(
                "SPACE requires an explicit bounded guest account name".into(),
            ));
        }
        path(&supervisor)?;
        let bytes = token.expose();
        if bytes.is_empty()
            || bytes.len() > 16 * 1024
            || !bytes.iter().all(|v| matches!(v, 0x21..=0x7e))
        {
            return Err(Error::InvalidInput(
                "invalid SPACE management credential".into(),
            ));
        }
        let client = Client::builder()
            .redirect(Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .build()
            .map_err(|_| Error::State("SPACE HTTP client initialization failed".into()))?;
        Ok(Self {
            api,
            data,
            token,
            user,
            supervisor,
            timeout: Duration::from_secs(30),
            client,
        })
    }
    /// One client RPC deadline, shared by create and all streamed frames. A
    /// private operation shares it across prepare/upload/invoke/scoped cleanup.
    /// Expiration is uncertainty, never proof that native work stopped.
    pub fn with_rpc_timeout(mut self, timeout: Duration) -> Result<Self> {
        if timeout < Duration::from_millis(1) || timeout > Duration::from_secs(120) {
            return Err(Error::InvalidInput(
                "SPACE RPC timeout must be 1ms..120s".into(),
            ));
        }
        self.timeout = timeout;
        Ok(self)
    }
}
pub(super) fn origin(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| Error::InvalidInput("invalid SPACE origin".into()))?;
    let loopback = url
        .host_str()
        .and_then(|host| host.trim_matches(['[', ']']).parse::<IpAddr>().ok())
        .is_some_and(|ip| ip.is_loopback());
    if !(url.scheme() == "https" || url.scheme() == "http" && loopback)
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(Error::InvalidInput(
            "SPACE requires an HTTPS origin or literal loopback HTTP origin".into(),
        ));
    }
    Ok(url)
}
pub(super) fn path(value: &str) -> Result<()> {
    if value.len() > 16 * 1024
        || !value.starts_with('/')
        || value == "/"
        || value.contains('\\')
        || value.contains('\u{fffd}')
        || value.chars().any(char::is_control)
        || value
            .split('/')
            .skip(1)
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(Error::InvalidInput(
            "SPACE requires an exact normalized absolute POSIX path".into(),
        ));
    }
    Ok(())
}
pub(super) fn sandbox(value: &str) -> Result<()> {
    if !value.strip_prefix("sbx_").is_some_and(canonical_uuid) {
        return Err(Error::InvalidInput("invalid SPACE sandbox identity".into()));
    }
    Ok(())
}
pub(super) fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
/// Attach to an existing sandbox and exact caller-chosen physical project path.
/// Its private control namespace is sibling `parent/.workspace-sdk/<WorkspaceId>`.
/// Creation/deletion of the SPACE resource and resource scheduling are unsupported.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpaceOpenOptions {
    pub sandbox_id: String,
    pub root: String,
}
impl SpaceOpenOptions {
    pub fn new(sandbox_id: impl Into<String>, root: impl Into<String>) -> Self {
        Self {
            sandbox_id: sandbox_id.into(),
            root: root.into(),
        }
    }
    pub(super) fn validate(&self) -> Result<()> {
        sandbox(&self.sandbox_id)?;
        path(&self.root)?;
        if self
            .root
            .split('/')
            .any(|part| part.eq_ignore_ascii_case(".workspace-sdk"))
        {
            return Err(Error::InvalidInput(
                "SPACE project root cannot enter the reserved SDK namespace".into(),
            ));
        }
        Ok(())
    }
}
