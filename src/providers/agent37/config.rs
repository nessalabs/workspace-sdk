//! Where an Agent37 workspace is reached, and the key that reaches it.
use super::failure::Agent37Failure;

/// The Sandbox API every workspace is served from.
pub const DEFAULT_API: &str = "https://api.agent37.com/v1";
/// Where an instance's SSH server is reached: a WebSocket on its port
/// 22022, authenticated by the workspace key. `{id}` is the instance.
pub const DEFAULT_TUNNEL: &str = "wss://{id}-22022.agent37.app/";

/// A workspace API key (`sk_live_…`). It is never printed: `Debug` shows
/// only that one is held.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);
impl ApiKey {
    /// # Errors
    /// [`Agent37Failure::InvalidInput`] for a key that is empty, too long,
    /// or holds anything but visible ASCII, which no header could carry.
    pub fn new(key: impl Into<String>) -> Result<Self, Agent37Failure> {
        let key = key.into();
        if key.is_empty() || key.len() > 512 || !key.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(Agent37Failure::InvalidInput);
        }
        Ok(Self(key))
    }
    pub(super) fn expose(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiKey(..)")
    }
}

/// One Agent37 workspace: its key, and where its API and tunnels are.
#[derive(Clone, Debug)]
pub struct Agent37Config {
    key: ApiKey,
    api: String,
    tunnel: String,
}
impl Agent37Config {
    /// The workspace `key` opens, on Agent37's own API and tunnels.
    pub fn new(key: ApiKey) -> Self {
        Self {
            key,
            api: DEFAULT_API.into(),
            tunnel: DEFAULT_TUNNEL.into(),
        }
    }
    /// Another API base, such as a test server's.
    ///
    /// # Errors
    /// [`Agent37Failure::InvalidInput`] unless it is an `https` URL, or an
    /// `http` one on this machine's loopback: the key is never sent in
    /// clear text across a network.
    pub fn with_api_base(mut self, base: &str) -> Result<Self, Agent37Failure> {
        if !(base.starts_with("https://") || loopback(base, "http://")) {
            return Err(Agent37Failure::InvalidInput);
        }
        self.api = base.trim_end_matches('/').into();
        Ok(self)
    }
    /// Another tunnel address, with `{id}` where the instance goes, such as
    /// a test server's.
    ///
    /// # Errors
    /// [`Agent37Failure::InvalidInput`] unless it is a `wss` URL, or a `ws`
    /// one on this machine's loopback, holding `{id}` once.
    pub fn with_tunnel(mut self, tunnel: &str) -> Result<Self, Agent37Failure> {
        if !(tunnel.starts_with("wss://") || loopback(tunnel, "ws://"))
            || tunnel.matches("{id}").count() != 1
        {
            return Err(Agent37Failure::InvalidInput);
        }
        self.tunnel = tunnel.into();
        Ok(self)
    }
    pub(super) fn key(&self) -> &ApiKey {
        &self.key
    }
    pub(super) fn api_base(&self) -> &str {
        &self.api
    }
    /// The tunnel to `instance`, an id already checked.
    pub(super) fn tunnel_to(&self, instance: &str) -> String {
        self.tunnel.replace("{id}", instance)
    }
}

/// Whether `url` has `scheme` and names a loopback address as its host.
fn loopback(url: &str, scheme: &str) -> bool {
    url.starts_with(scheme)
        && reqwest::Url::parse(url).is_ok_and(|url| {
            url.username().is_empty()
                && url.password().is_none()
                && url.host_str().is_some_and(|host| {
                    host == "localhost"
                        || host
                            .trim_start_matches('[')
                            .trim_end_matches(']')
                            .parse::<std::net::IpAddr>()
                            .is_ok_and(|address| address.is_loopback())
                })
        })
}
