//! Service branding defaults. Wire schemas and durable receipt names live in their owners.
/// Published Boat API root; callers can override it through client configuration.
pub const BASE_URL: &str = "https://boat.dev/api/v1";
/// API-key environment variable.
pub const API_KEY_ENV: &str = "BOAT_API_KEY";
/// Optional API-root override environment variable.
pub const BASE_URL_ENV: &str = "BOAT_BASE_URL";
/// Optional organization environment variable.
pub const ORG_ENV: &str = "BOAT_ORG";
/// Organization request header (environment APIs require a query scope instead).
pub const ORG_HEADER: &str = "X-Boat-Org";
/// Client product identifier used in the HTTP user agent.
pub const CLIENT_NAME: &str = "boat_client";
/// Resource route prefix owned by the current public protocol.
pub(crate) const SANDBOX_PATH: &str = "/sandboxes";
