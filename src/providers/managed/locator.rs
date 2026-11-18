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
    let locator: Locator = decode_reference(reference, provider)?;
    validate_remote_path(&locator.root)?;
    validate_remote_path(&locator.control)?;
    Ok(locator)
}

/// A reference's locator, in whatever shape its provider keeps, once its
/// version and provider are this registration's.
pub(in crate::providers) fn decode_reference<T: serde::de::DeserializeOwned>(
    reference: &WorkspaceRef,
    provider: &ProviderId,
) -> Result<T> {
    if reference.version != 1 || &reference.provider != provider {
        return Err(Error::InvalidInput(
            "workspace reference version or provider does not match".into(),
        ));
    }
    reference.locator.decode()
}

/// The control root of an attached workspace: `.workspace-sdk/{id}` beside
/// its root, so the SDK's state never sits inside the files it serves.
/// `check` is the provider's own rule for a path it can address.
#[cfg(any(feature = "space", feature = "agent37"))]
pub(in crate::providers) fn control_beside(
    root: &str,
    id: &WorkspaceId,
    check: impl Fn(&str) -> Result<()>,
) -> Result<String> {
    let parent = root.rsplit_once('/').map_or("", |(parent, _)| parent);
    let control = format!("{parent}/.workspace-sdk/{id}");
    check(&control)?;
    Ok(control)
}

/// Refuse a stored locator unless it is the one this registration would
/// make for the same workspace now.
#[cfg(any(feature = "space", feature = "agent37"))]
pub(in crate::providers) fn require_same<L: PartialEq>(
    stored: &L,
    expected: &L,
    provider: &str,
) -> Result<()> {
    if stored == expected {
        Ok(())
    } else {
        Err(Error::InvalidInput(format!(
            "{provider} locator does not match the configured identity"
        )))
    }
}
