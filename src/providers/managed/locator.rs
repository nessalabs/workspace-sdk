use super::validate_remote_path;
use crate::{domain::*, Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::providers) struct Locator {
    pub root: String,
    pub control: String,
    pub ownership: Ownership,
    pub target: String,
}
pub(in crate::providers) fn decode(
    reference: &WorkspaceRef,
    provider: &ProviderId,
) -> Result<Locator> {
    if reference.version != 1 || &reference.provider != provider {
        return Err(Error::InvalidInput(
            "workspace reference version or provider does not match".into(),
        ));
    }
    let locator: Locator = reference.locator.decode()?;
    validate_remote_path(&locator.root)?;
    validate_remote_path(&locator.control)?;
    Ok(locator)
}
