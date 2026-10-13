use std::env;
use std::fmt;
use std::time::Duration;

use url::Url;

use crate::{
    error::{Error, Result},
    service,
};

/// Authentication, organization scope, HTTP timeouts, retry policy and response limits.
///
/// Timeouts follow common Rust API-client practice (e.g. octocrab / reqwest):
/// a short **connect** budget and a separate **request** budget. Long-running
/// `command` calls extend the request timeout to cover `timeoutSeconds` plus slack.
#[derive(Clone)]
pub struct ClientConfig {
    /// Automatic GET retry policy. Mutations are never automatically retried.
    pub retry: RetryConfig,
    pub base_path: String,
    pub access_token: String,
    /// Optional org / team wallet scope (`X-Boat-Org`).
    pub org: Option<String>,
    /// TCP connect timeout (default 10s).
    pub connect_timeout: Duration,
    /// Default per-request timeout for API calls (default 60s).
    pub request_timeout: Duration,
    /// Added on top of a command's `timeout_seconds` for the HTTP layer (default 15s).
    pub command_timeout_slack: Duration,
    /// Sent as `User-Agent` (default `boat_client/{crate version}`).
    pub user_agent: String,
    /// Maximum buffered response body (default 64 MiB), including snapshot files.
    pub max_response_bytes: usize,
}

impl fmt::Debug for ClientConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientConfig")
            .field("retry", &self.retry)
            .field("base_path", &self.base_path)
            .field("access_token", &redact_secret(&self.access_token))
            .field("org", &self.org)
            .field("connect_timeout", &self.connect_timeout)
            .field("request_timeout", &self.request_timeout)
            .field("command_timeout_slack", &self.command_timeout_slack)
            .field("user_agent", &self.user_agent)
            .field("max_response_bytes", &self.max_response_bytes)
            .finish()
    }
}

impl ClientConfig {
    pub const DEFAULT_BASE_PATH: &'static str = service::BASE_URL;
    pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
    pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
    pub const DEFAULT_COMMAND_TIMEOUT_SLACK: Duration = Duration::from_secs(15);

    pub fn new(access_token: impl Into<String>) -> Result<Self> {
        let access_token = access_token.into();
        validate_token(&access_token)?;
        let base_path = Self::DEFAULT_BASE_PATH.to_string();
        validate_base_path(&base_path)?;
        Ok(Self {
            retry: RetryConfig::default(),
            base_path,
            access_token,
            org: None,
            connect_timeout: Self::DEFAULT_CONNECT_TIMEOUT,
            request_timeout: Self::DEFAULT_REQUEST_TIMEOUT,
            command_timeout_slack: Self::DEFAULT_COMMAND_TIMEOUT_SLACK,
            max_response_bytes: 64 * 1024 * 1024,
            user_agent: format!("{}/{}", service::CLIENT_NAME, env!("CARGO_PKG_VERSION")),
        })
    }

    /// Load from `BOAT_API_KEY` (required) and optional `BOAT_BASE_URL` / `BOAT_ORG`.
    pub fn from_env() -> Result<Self> {
        let access_token = env::var(service::API_KEY_ENV).map_err(|_| {
            Error::Config("BOAT_API_KEY is not set (create one with `boat api-key create`)".into())
        })?;
        let mut cfg = Self::new(access_token)?;
        if let Ok(base) = env::var(service::BASE_URL_ENV) {
            if !base.is_empty() {
                cfg = cfg.with_base_path(base)?;
            }
        }
        if let Ok(org) = env::var(service::ORG_ENV) {
            if !org.is_empty() {
                cfg.org = Some(org);
            }
        }
        Ok(cfg)
    }

    pub fn with_base_path(mut self, base_path: impl Into<String>) -> Result<Self> {
        let base_path = base_path.into();
        validate_base_path(&base_path)?;
        self.base_path = base_path.trim_end_matches('/').to_string();
        Ok(self)
    }

    pub fn with_org(mut self, org: impl Into<String>) -> Self {
        self.org = Some(org.into());
        self
    }

    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    pub fn with_command_timeout_slack(mut self, slack: Duration) -> Self {
        self.command_timeout_slack = slack;
        self
    }

    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    pub fn with_max_response_bytes(mut self, max_bytes: usize) -> Self {
        self.max_response_bytes = max_bytes;
        self
    }

    /// Configure automatic GET retries. Validated when constructing `Api`.
    pub fn with_retry(mut self, retry: RetryConfig) -> Self {
        self.retry = retry;
        self
    }

    /// HTTP timeout for a `command` call given the in-sandbox `timeout_seconds`.
    pub fn http_timeout_for_command(&self, timeout_seconds: Option<u32>) -> Duration {
        // Server default is 30s when omitted.
        let cmd = Duration::from_secs(u64::from(timeout_seconds.unwrap_or(30)));
        cmd.saturating_add(self.command_timeout_slack)
            .max(self.request_timeout)
    }
}

fn validate_token(token: &str) -> Result<()> {
    if token.is_empty() || token.chars().all(char::is_whitespace) {
        return Err(Error::Config("access token must not be empty".into()));
    }
    Ok(())
}

pub(crate) fn validate_base_path(base: &str) -> Result<()> {
    let parsed =
        Url::parse(base).map_err(|e| Error::Config(format!("invalid base_path `{base}`: {e}")))?;
    match parsed.scheme() {
        "https" => {}
        "http" => {
            let loopback = match parsed.host() {
                Some(url::Host::Domain("localhost")) => true,
                Some(url::Host::Ipv4(address)) => address.is_loopback(),
                Some(url::Host::Ipv6(address)) => address.is_loopback(),
                _ => false,
            };
            if !loopback {
                return Err(Error::Config(format!(
                    "base_path must use https (http only allowed for localhost), got `{base}`"
                )));
            }
        }
        other => {
            return Err(Error::Config(format!(
                "base_path must be http(s), got {other}"
            )));
        }
    }
    if parsed.host_str().is_none() {
        return Err(Error::Config("base_path must include a host".into()));
    }
    Ok(())
}

fn redact_secret(secret: &str) -> String {
    // Never echo key material in logs — length only.
    format!("<redacted len={}>", secret.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_token() {
        let cfg = ClientConfig::new("box_secret_value_here").unwrap();
        let s = format!("{cfg:?}");
        assert!(s.contains("<redacted len="));
        assert!(!s.contains("box_secret"));
        assert!(!s.contains("secret_value"));
    }

    #[test]
    fn rejects_empty_token() {
        assert!(ClientConfig::new("").is_err());
    }

    #[test]
    fn rejects_remote_http_base_path() {
        let cfg = ClientConfig::new("box_test_key").unwrap();
        assert!(cfg
            .clone()
            .with_base_path("http://evil.example/api")
            .is_err());
        assert!(cfg.with_base_path("http://127.0.0.1:8080").is_ok());
    }

    #[test]
    fn command_http_timeout_covers_command_budget() {
        let cfg = ClientConfig::new("box_test_key").unwrap();
        let t = cfg.http_timeout_for_command(Some(120));
        assert!(t >= Duration::from_secs(135));
    }
}

/// Exponential backoff for safe GET requests. `max_attempts = 1` disables retries.
#[derive(Clone, Debug)]
pub struct RetryConfig {
    /// Total attempts including the initial request (default 3, minimum 1).
    pub max_attempts: u32,
    /// Initial backoff before jitter (default 200 ms).
    pub initial_delay: Duration,
    /// Maximum local backoff including jitter (default 30 s).
    /// A server's Retry-After may exceed this; it is never shortened.
    pub max_delay: Duration,
    /// Exponential multiplier (default 2, minimum 1).
    pub multiplier: u32,
    /// Add uniform jitter from zero through the current base delay (default true).
    pub jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(30),
            multiplier: 2,
            jitter: true,
        }
    }
}

impl RetryConfig {
    pub fn disabled() -> Self {
        Self {
            max_attempts: 1,
            ..Self::default()
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.max_attempts == 0 || self.multiplier == 0 || self.initial_delay > self.max_delay {
            return Err(Error::Config(
                "retry requires max_attempts >= 1, multiplier >= 1, and initial_delay <= max_delay"
                    .into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn delay(&self, retry_index: u32) -> Duration {
        let mut base = self.initial_delay;
        if self.multiplier > 1 && !base.is_zero() {
            for _ in 0..retry_index {
                base = base.saturating_mul(self.multiplier).min(self.max_delay);
                if base == self.max_delay {
                    break;
                }
            }
        }
        let jitter = if self.jitter {
            Duration::from_nanos(fastrand::u64(
                0..=base.as_nanos().min(u64::MAX as u128) as u64,
            ))
        } else {
            Duration::ZERO
        };
        base.saturating_add(jitter).min(self.max_delay)
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    #[test]
    fn exponential_backoff_caps_and_handles_extreme_indices() {
        let retry = RetryConfig {
            initial_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(25),
            jitter: false,
            ..Default::default()
        };
        assert_eq!(retry.delay(0), Duration::from_millis(10));
        assert_eq!(retry.delay(1), Duration::from_millis(20));
        assert_eq!(retry.delay(2), Duration::from_millis(25));
        assert_eq!(retry.delay(u32::MAX), Duration::from_millis(25));
        let constant = RetryConfig {
            multiplier: 1,
            ..retry.clone()
        };
        assert_eq!(constant.delay(u32::MAX), Duration::from_millis(10));
        let zero = RetryConfig {
            initial_delay: Duration::ZERO,
            ..retry
        };
        assert_eq!(zero.delay(u32::MAX), Duration::ZERO);
    }
    #[test]
    fn jitter_is_bounded_by_base_and_maximum() {
        let retry = RetryConfig::default();
        for index in 0..20 {
            for _ in 0..100 {
                let delay = retry.delay(index);
                assert!(delay >= retry.initial_delay);
                assert!(delay <= retry.max_delay);
                if index == 0 {
                    assert!(delay <= retry.initial_delay * 2);
                }
            }
        }
    }
    #[test]
    fn invalid_retry_configuration_fails_before_network_io() {
        for retry in [
            RetryConfig {
                max_attempts: 0,
                ..Default::default()
            },
            RetryConfig {
                multiplier: 0,
                ..Default::default()
            },
            RetryConfig {
                max_delay: Duration::ZERO,
                ..Default::default()
            },
        ] {
            assert!(matches!(
                crate::Api::new(ClientConfig::new("fixture-only").unwrap().with_retry(retry)),
                Err(Error::Config(_))
            ));
        }
        assert!(crate::Api::new(
            ClientConfig::new("fixture-only")
                .unwrap()
                .with_retry(RetryConfig::disabled())
        )
        .is_ok());
    }
}
