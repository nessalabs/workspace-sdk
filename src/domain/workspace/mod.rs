//! Workspace-management context: identity, access, resources, and lifecycle.
//!
//! A [`crate::Workspace`] is the identity-bearing application handle: its
//! identity survives while the target’s state changes. [`WorkspaceId`] and
//! [`WorkspaceRef`] are value objects used to identify and reconnect to it.
//! [`BackendDescriptor`] and [`WorkspaceState`] are observations, not live entities.
//! [`OpenRequest`], [`ReadFile`], and [`WriteFile`] describe requested actions.
//!
//! Value-object semantics mean that a value describes a fact and can be replaced;
//! they do not mean that every public Rust field is immutable. Validated identity
//! and path types keep their inner values private. Provider side effects belong
//! behind [`crate::ports::IBackend`] and [`crate::ports::IProvider`].

pub mod deletion;
pub mod descriptor;
pub mod edit_session;
pub mod file_browser;
pub mod file_tree;
pub mod files;
pub mod identity;
pub mod open;
pub mod path;
pub mod ports;
pub mod provider_data;
pub mod resources;

pub use deletion::{DeleteOperation, DeleteState, OperationId, OperationRef};
pub use descriptor::{BackendDescriptor, Capabilities, Isolation, Ownership, WorkspaceState};
pub use edit_session::{EditChange, EditRevision};
pub use file_browser::*;
pub use file_tree::{TreeExportReceipt, TreeImportReceipt, TreeTransfer};
pub use files::{FileChunk, ReadFile, WriteFile};
pub use identity::{ProviderId, WorkspaceId, WorkspaceRef};
pub use open::{OpenRequest, ResourceRequirements};
pub use path::WorkspacePath;
pub use ports::{PortAccess, PortPolicy, PortRequest};
pub use provider_data::{ProviderOptions, WorkspaceLocator};
pub use resources::{HostGpu, HostResources};
