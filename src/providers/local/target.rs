use crate::providers::{
    managed::{self, ITarget, Locator},
    ICommandRunner, NativeCommand,
};
use crate::supervisor::protocol::{
    SupervisorContext, SupervisorOperation, SupervisorRequest, SupervisorValue,
};
use crate::{domain::*, Error, Result};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{ErrorKind, Write},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

pub(super) struct LocalTarget {
    pub(super) supervisor: PathBuf,
    pub(super) runner: Arc<dyn ICommandRunner>,
    pub(super) locator: Locator,
    pub(super) reference: WorkspaceRef,
    pub(super) base: PathBuf,
}
impl LocalTarget {
    fn roots_absent(&self) -> Result<bool> {
        for path in [&self.locator.root, &self.locator.control] {
            match fs::symlink_metadata(path) {
                Ok(_) => return Ok(false),
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(Error::Io(error)),
            }
        }
        Ok(true)
    }
    fn request(&self, operation: SupervisorOperation) -> SupervisorRequest {
        SupervisorRequest::new(
            SupervisorContext {
                workspace_id: self.reference.id.clone(),
                root: self.locator.root.clone(),
                control: self.locator.control.clone(),
                ownership: self.locator.ownership.clone(),
            },
            operation,
        )
    }
    fn receipt(&self, operation: &OperationRef) -> PathBuf {
        self.base
            .join(".deletions")
            .join(format!("{}.json", operation.id))
    }
    fn save_receipt(&self, deletion: &DeleteOperation) -> Result<()> {
        let path = self.receipt(&deletion.reference);
        let parent = path.parent().expect("receipt parent");
        fs::create_dir_all(parent)?;
        if fs::symlink_metadata(parent)?.file_type().is_symlink() {
            return Err(Error::InvalidInput(
                "deletion receipt directory must not be a symlink".into(),
            ));
        }
        let mut pending = tempfile::NamedTempFile::new_in(parent)?;
        pending.write_all(&serde_json::to_vec(deletion)?)?;
        pending.as_file().sync_all()?;
        pending
            .persist(&path)
            .map_err(|error| Error::Io(error.error))?;
        File::open(parent)?.sync_all()?;
        Ok(())
    }
}
#[async_trait]
impl ITarget for LocalTarget {
    async fn invoke(&self, request: SupervisorRequest) -> Result<SupervisorValue> {
        let output = self
            .runner
            .run(NativeCommand {
                program: self
                    .supervisor
                    .to_str()
                    .ok_or_else(|| {
                        Error::InvalidInput("supervisor executable path must be UTF-8".into())
                    })?
                    .into(),
                args: vec![],
                stdin: serde_json::to_vec(&request)?,
                timeout: Duration::from_secs(30),
            })
            .await?;
        managed::response(&output.stdout, output.success())
    }
    async fn port(&self, request: PortRequest) -> Result<PortAccess> {
        if request.policy != PortPolicy::Private {
            return Err(Error::Unsupported("local authenticated hosting".into()));
        }
        // URL is loopback access; binding of the actual server remains caller-owned.
        Ok(PortAccess {
            url: format!("http://127.0.0.1:{}", request.port),
            policy: PortPolicy::Private,
        })
    }
    async fn stop(&self) -> Result<()> {
        self.invoke(self.request(SupervisorOperation::StopOwnedRuns))
            .await?;
        Ok(())
    }
    async fn destroy(&self, reference: &WorkspaceRef) -> Result<DeleteOperation> {
        let operation = OperationRef {
            workspace: reference.clone(),
            id: OperationId::new(format!(
                "local-delete-{:x}",
                Sha256::digest(serde_json::to_vec(reference)?)
            ))?,
        };
        let mut deletion = DeleteOperation {
            reference: operation,
            state: DeleteState::Processing,
            detail: None,
        };
        self.save_receipt(&deletion)?;
        if self.roots_absent()? {
            deletion.state = DeleteState::Completed;
            self.save_receipt(&deletion)?;
            return Ok(deletion);
        }
        match self
            .invoke(self.request(SupervisorOperation::DestroyOwnedFiles))
            .await
        {
            Ok(_) => {}
            Err(Error::Timeout(_)) => {
                deletion.state = DeleteState::Unknown;
                deletion.detail = Some(
                    "Workspace deletion outcome is unknown. Inspect this receipt before retrying."
                        .into(),
                );
                self.save_receipt(&deletion)?;
                return Ok(deletion);
            }
            Err(_) => {
                deletion.state = DeleteState::Blocked;
                deletion.detail = Some("Workspace deletion was blocked. Check ownership and reconcile any lost managed runs before retrying.".into());
                self.save_receipt(&deletion)?;
                return Ok(deletion);
            }
        }
        // Only remove the empty owned task parent; arbitrary additional files are retained.
        let _ = fs::remove_dir(self.base.join(reference.id.as_str()));
        deletion.state = DeleteState::Completed;
        self.save_receipt(&deletion)?;
        Ok(deletion)
    }
    async fn inspect_delete(&self, operation: &OperationRef) -> Result<DeleteOperation> {
        let mut receipt: DeleteOperation =
            serde_json::from_slice(&fs::read(self.receipt(operation))?)?;
        if receipt.reference.workspace.id != operation.workspace.id {
            return Err(Error::InvalidInput(
                "delete receipt belongs to a different workspace".into(),
            ));
        }
        if receipt.state != DeleteState::Completed && self.roots_absent()? {
            receipt.state = DeleteState::Completed;
            receipt.detail = None;
            self.save_receipt(&receipt)?;
        }
        Ok(receipt)
    }
}
