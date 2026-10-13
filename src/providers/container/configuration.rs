use crate::providers::managed::validate_remote_path;
use crate::providers::SupervisorDeployment;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
/// Engine and image settings shared by a [`super::ContainerProvider`].
///
/// New workspaces use dedicated containers and named volumes. The provider
/// does not automatically mount host project directories or credentials.
#[derive(Clone, Debug)]
pub struct ContainerConfig {
    /// Local Docker/Podman-compatible CLI executable, such as `docker` or `podman`.
    ///
    /// Engine authentication and connection context are configured by the caller.
    pub engine: String,
    /// Explicit absolute task namespace inside each target container.
    pub root: String,
    /// Container image reference used when creating a new workspace.
    ///
    /// The image must provide a POSIX `sh` and the configured native supervisor or upload prerequisites.
    /// It is not used to replace the image of an attached container.
    pub image: String,
    /// Explicit native supervisor deployment inside the container.
    pub supervisor: SupervisorDeployment,
}
impl ContainerConfig {
    /// Select the engine and creation image, with an explicitly installed or supplied native supervisor.
    pub fn new(
        engine: impl Into<String>,
        image: impl Into<String>,
        root: impl Into<String>,
    ) -> Self {
        Self {
            engine: engine.into(),
            root: root.into(),
            image: image.into(),
            supervisor: SupervisorDeployment::default(),
        }
    }
    pub(super) fn validate(&self) -> Result<()> {
        if self.engine.is_empty()
            || self.image.is_empty()
            || self.image.starts_with('-')
            || self.engine.contains('\0')
            || self.image.contains('\0')
        {
            return Err(Error::InvalidInput(
                "container engine and image are required".into(),
            ));
        }
        validate_remote_path(&self.root)?;
        self.supervisor.validate()?;
        Ok(())
    }
}

/// Per-workspace container options serialized into [`crate::OpenRequest`] `config`.
///
/// The default creates an SDK-owned container and named volume. Attachment
/// instead requires an existing running container with the configured supervisor.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContainerOpenOptions {
    /// Existing container name or identifier to attach to.
    ///
    /// `None` creates a container named `workspace-sdk-<workspace_id>`.
    /// Attachments use `<root>/<workspace_id>/project` and a sibling
    /// private `control` directory; the SDK does not start, stop, or delete the
    /// attached container.
    pub container_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_cannot_inject_engine_flags() {
        assert!(ContainerConfig::new("docker", "--privileged", "/workspace")
            .validate()
            .is_err());
        assert!(ContainerConfig::new("podman", "image", "/workspace")
            .validate()
            .is_ok());
    }
}
