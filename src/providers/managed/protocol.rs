use crate::supervisor::protocol::{SupervisorResponse, SupervisorValue};
use crate::{Error, ProviderOptions, ResourceRequirements, Result};

pub(in crate::providers) const MAX_CHUNK: usize = 256 * 1024;
pub(in crate::providers) fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
pub(in crate::providers) fn validate_remote_path(value: &str) -> Result<()> {
    if !value.starts_with('/')
        || value == "/"
        || value.contains('\0')
        || value.contains('\\')
        || value.split('/').any(|p| p == ".." || p == ".")
    {
        return Err(Error::InvalidInput(
            "expected an absolute normalized POSIX target directory".into(),
        ));
    }
    Ok(())
}
pub(in crate::providers) fn require_no_resources(resources: &ResourceRequirements) -> Result<()> {
    if resources != &ResourceRequirements::default() {
        Err(Error::Unsupported(
            "this adapter cannot provision resource requirements on an existing host".into(),
        ))
    } else {
        Ok(())
    }
}
/// Refuse create options for an adapter whose create takes none, so a caller
/// cannot pass attach-shaped options to `create` and get a silent default.
pub(in crate::providers) fn require_no_create_options(
    options: &ProviderOptions,
    adapter: &str,
) -> Result<()> {
    if options.is_empty() {
        Ok(())
    } else {
        Err(Error::InvalidInput(format!(
            "{adapter} create takes no options; use attach for an existing target"
        )))
    }
}
pub(in crate::providers) fn response(stdout: &str, success: bool) -> Result<SupervisorValue> {
    if !success {
        return Err(Error::Provider{provider:"target".into(),message:"native supervisor invocation failed; verify the configured executable and target access (raw output redacted)".into()});
    }
    let response: SupervisorResponse = serde_json::from_str(stdout.trim()).map_err(|_| {
        Error::State("invalid native supervisor response (raw output redacted)".into())
    })?;
    response.into_result()
}
