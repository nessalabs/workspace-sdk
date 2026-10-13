use crate::Error;
use sandbox_client::Error as SandboxError;
pub(super) fn error(error: SandboxError) -> Error {
    Error::Provider {
        provider: super::branding::NAME.into(),
        message: error.to_string(),
    }
}
