use crate::{domain::*, Result};
use sandbox_client::DeletionOperation;

pub(super) fn deletion(
    workspace: &WorkspaceRef,
    value: DeletionOperation,
) -> Result<DeleteOperation> {
    let state = match value.status.as_str() {
        "pending" => DeleteState::Pending,
        "processing" => DeleteState::Processing,
        "blocked" => DeleteState::Blocked,
        "completed" => DeleteState::Completed,
        _ => DeleteState::Unknown,
    };
    Ok(DeleteOperation {
        reference: OperationRef {
            workspace: workspace.clone(),
            id: OperationId::new(value.id)?,
        },
        state,
        detail: None,
    })
}
