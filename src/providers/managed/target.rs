use crate::ports::{IConnection, SocketForward};
use crate::supervisor::protocol::{SupervisorRequest, SupervisorValue};
use crate::{domain::*, Error, Result};
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub(in crate::providers) trait ITarget: Send + Sync {
    async fn invoke(&self, request: SupervisorRequest) -> Result<SupervisorValue>;
    async fn ensure(&self) -> Result<()> {
        Ok(())
    }
    async fn state(&self) -> Result<WorkspaceState> {
        Ok(WorkspaceState::Ready)
    }
    async fn port(&self, _request: PortRequest) -> Result<PortAccess> {
        Err(Error::Unsupported("ports".into()))
    }
    async fn stop(&self) -> Result<()> {
        Err(Error::Unsupported("stop".into()))
    }
    async fn destroy(&self, _reference: &WorkspaceRef) -> Result<DeleteOperation> {
        Err(Error::Unsupported("destroy".into()))
    }
    async fn inspect_delete(&self, _reference: &OperationRef) -> Result<DeleteOperation> {
        Err(Error::Unsupported("delete status".into()))
    }
    async fn connection(&self, _forward: Option<SocketForward>) -> Result<Arc<dyn IConnection>> {
        Err(Error::Unsupported("live connections".into()))
    }
}
