//! Request construction, safe diagnostics, retries, bounded reads and decoding.
use super::Api;
use crate::{service, ApiErrorBody, Error, Result};
use reqwest::{Method, Request, RequestBuilder, StatusCode};
use serde::{de::DeserializeOwned, Serialize};
use std::time::{Duration, Instant};
use tracing::Instrument;

const ERROR_BODY_MAX: usize = 2_048;

/// Private builder: endpoint methods supply only their operation and wire inputs.
pub(super) struct ApiRequest<'a> {
    api: &'a Api,
    operation: &'static str,
    request: RequestBuilder,
}

impl Api {
    pub(super) fn request(
        &self,
        operation: &'static str,
        method: Method,
        path: &str,
    ) -> ApiRequest<'_> {
        self.request_with_org(operation, method, path, None)
    }

    pub(super) fn request_with_org(
        &self,
        operation: &'static str,
        method: Method,
        path: &str,
        org: Option<&str>,
    ) -> ApiRequest<'_> {
        let mut request = self
            .http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(&self.config.access_token)
            .header("Accept", "application/json");
        if let Some(org) = org.or(self.config.org.as_deref()) {
            if path == "/environments" || path.starts_with("/environments/") {
                request = request.query(&[("org", org)]);
            } else {
                request = request.header(service::ORG_HEADER, org);
            }
        }
        ApiRequest {
            api: self,
            operation,
            request,
        }
    }

    async fn execute<T>(
        &self,
        builder: RequestBuilder,
        operation: &'static str,
        decode: impl FnOnce(Vec<u8>) -> Result<T>,
    ) -> Result<T> {
        let request = builder.build().map_err(|error| {
            tracing::debug!(target: "boat_client::http", operation, "request construction failed");
            Error::from(error)
        })?;
        // Only explicitly supplied static operation names are logged. Never record
        // a Request, URL, header, body, server error text, or user-provided identifier.
        let span = tracing::debug_span!(target: "boat_client::http", "request",
            operation, method = %request.method());
        async {
            let start = Instant::now();
            tracing::debug!(target: "boat_client::http", "request started");
            let result = self.execute_bytes(&request, operation).await.and_then(decode);
            match &result {
                Ok(_) => tracing::debug!(target: "boat_client::http", elapsed_ms = millis(start.elapsed()), "request completed"),
                Err(error) => tracing::debug!(target: "boat_client::http", elapsed_ms = millis(start.elapsed()),
                    error_kind = error_kind(error), status = ?error.status(), "request failed"),
            }
            result
        }.instrument(span).await
    }

    async fn execute_bytes(&self, request: &Request, operation: &'static str) -> Result<Vec<u8>> {
        // Derive safety from the actual method, so no endpoint can accidentally
        // opt a mutation into retries with a boolean flag.
        let retry_safe = request.method() == Method::GET;
        let max_attempts = if retry_safe {
            self.config.retry.max_attempts
        } else {
            1
        };
        for attempt in 0..max_attempts {
            let pending = request
                .try_clone()
                .ok_or_else(|| Error::Unexpected("request body is not retryable".into()))?;
            let start = Instant::now();
            let (result, retry_after, status) = self.send_once(pending).await;
            tracing::debug!(target: "boat_client::http", attempt = attempt + 1, max_attempts,
                status = ?status, elapsed_ms = millis(start.elapsed()),
                error_kind = result.as_ref().err().map(error_kind).unwrap_or("none"), "request attempt finished");
            match result {
                Ok(bytes) => return Ok(bytes),
                Err(error) if retry_safe && error.is_retryable() && attempt + 1 < max_attempts => {
                    let backoff = self.config.retry.delay(attempt);
                    let delay = retry_after.map(|d| d.max(backoff)).unwrap_or(backoff);
                    if delay > self.config.request_timeout {
                        tracing::debug!(target: "boat_client::retry", operation, method = %request.method(), attempt = attempt + 1,
                            delay_ms = millis(delay), "retry delay exceeds request timeout; returning error");
                        return Err(error);
                    }
                    tracing::debug!(target: "boat_client::retry", operation, method = %request.method(), attempt = attempt + 1,
                        next_attempt = attempt + 2, max_attempts, delay_ms = millis(delay),
                        status = ?error.status(), error_kind = error_kind(&error), "retrying GET request");
                    tokio::time::sleep(delay).await;
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("validated configuration guarantees at least one attempt")
    }

    async fn send_once(
        &self,
        request: Request,
    ) -> (Result<Vec<u8>>, Option<Duration>, Option<u16>) {
        let mut response = match self.http.execute(request).await {
            Ok(response) => response,
            Err(error) => return (Err(error.into()), None, None),
        };
        let status = response.status();
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_retry_after);
        let limit = self.config.max_response_bytes;
        let result = async {
            if response.content_length().is_some_and(|n| n > limit as u64) {
                return Err(Error::ResponseTooLarge { limit });
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                if chunk.len() > limit.saturating_sub(bytes.len()) {
                    return Err(Error::ResponseTooLarge { limit });
                }
                bytes.extend_from_slice(&chunk);
            }
            if status.is_success() {
                Ok(bytes)
            } else {
                Err(map_error(status, &bytes, retry_after))
            }
        }
        .await;
        (result, retry_after, Some(status.as_u16()))
    }
}

impl ApiRequest<'_> {
    pub(super) fn query<Q: Serialize + ?Sized>(mut self, query: Option<&Q>) -> Self {
        if let Some(query) = query {
            self.request = self.request.query(query);
        }
        self
    }
    pub(super) fn json<B: Serialize + ?Sized>(mut self, body: Option<&B>) -> Self {
        self.request = match body {
            Some(body) => self.request.json(body),
            None => self.request.header("Content-Type", "application/json"),
        };
        self
    }
    pub(super) fn header(mut self, name: &'static str, value: Option<&str>) -> Self {
        if let Some(value) = value {
            self.request = self.request.header(name, value);
        }
        self
    }
    pub(super) fn timeout(mut self, timeout: Duration) -> Self {
        self.request = self.request.timeout(timeout);
        self
    }
    pub(super) async fn send<T: DeserializeOwned>(self) -> Result<T> {
        self.api
            .execute(self.request, self.operation, |bytes| {
                serde_json::from_slice(&bytes).map_err(Error::from)
            })
            .await
    }
    pub(super) async fn bytes(self) -> Result<Vec<u8>> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("*/*"),
        );
        self.api
            .execute(self.request.headers(headers), self.operation, Ok)
            .await
    }
}

fn millis(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}
fn error_kind(error: &Error) -> &'static str {
    match error {
        Error::Http(error) if error.is_timeout() => "timeout",
        Error::Http(error) if error.is_connect() => "connect",
        Error::Http(_) => "transport",
        Error::Api { .. } => "api",
        Error::HttpStatus { .. } => "http_status",
        Error::Serde(_) => "decode",
        Error::ResponseTooLarge { .. } => "response_too_large",
        _ => "client",
    }
}
fn parse_retry_after(value: &str) -> Option<Duration> {
    value
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
        .or_else(|| {
            httpdate::parse_http_date(value).ok().map(|date| {
                date.duration_since(std::time::SystemTime::now())
                    .unwrap_or_default()
            })
        })
}

fn map_error(status: StatusCode, bytes: &[u8], retry_after: Option<Duration>) -> Error {
    if let Ok(body) = serde_json::from_slice::<ApiErrorBody>(bytes) {
        let code = body
            .error
            .as_ref()
            .and_then(|e| e.code.clone())
            .or(body.code)
            .unwrap_or_else(|| "unknown".into());
        let message = body
            .error
            .as_ref()
            .and_then(|e| e.message.clone())
            .or(body.message)
            .unwrap_or_else(|| truncate_lossy(bytes));
        let details = body.error.and_then(|e| e.details).or(body.details);
        return Error::Api {
            retry_after,
            status: status.as_u16(),
            code,
            message: truncate_str(&message),
            request_id: body.request_id.unwrap_or_default(),
            details: details.map(Box::new),
        };
    }
    Error::HttpStatus {
        retry_after,
        status: status.as_u16(),
        body: truncate_lossy(bytes),
    }
}

fn truncate_lossy(bytes: &[u8]) -> String {
    truncate_str(&String::from_utf8_lossy(bytes))
}

fn truncate_str(s: &str) -> String {
    if s.len() <= ERROR_BODY_MAX {
        return s.to_string();
    }
    let mut end = ERROR_BODY_MAX;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod truncate_tests {
    #[test]
    fn truncate_respects_utf8_boundaries() {
        // "é" is 2 bytes in UTF-8; force a mid-character cut point.
        let mut bytes = Vec::new();
        while bytes.len() < super::ERROR_BODY_MAX - 1 {
            bytes.extend_from_slice("a".as_bytes());
        }
        bytes.extend_from_slice("é".as_bytes());
        let out = super::truncate_lossy(&bytes);
        assert!(out.ends_with('…'));
        assert!(out.is_char_boundary(out.len() - '…'.len_utf8()));
    }
}
