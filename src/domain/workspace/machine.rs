//! The saved record of one workspace's target: one place, one set of states.
use super::{BackendDescriptor, WorkspaceId};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The provider's idempotency key for creating one workspace's target.
///
/// It is derived only from the workspace ID, so every attempt to create the same
/// workspace, including one after a restart, asks the provider for the same
/// target instead of a second one.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProvisionKey(String);
impl ProvisionKey {
    /// The key for `workspace_id`.
    pub fn for_workspace(workspace_id: &WorkspaceId) -> Self {
        Self(format!(
            "workspace-sdk-{:x}",
            Sha256::digest(workspace_id.as_str().as_bytes())
        ))
    }
    /// The key as the provider receives it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a target was admitted under: the environment the provider reported,
/// checked against what the caller asked for.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    /// The provider's identifier for the environment.
    pub environment_id: String,
    /// The provider's identifier for the environment version.
    pub version_id: String,
    /// The version label the provider reported.
    pub version: String,
    /// The environment's name.
    pub name: String,
}

/// Why a target may never be used.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalKind {
    /// The target runs a different or missing environment or version.
    EnvironmentMismatch,
    /// The target is waiting to be wiped.
    PendingWipe,
    /// The provider could not prove the target meets the request.
    NotAdmitted,
    /// Any other provider refusal.
    Other,
}

/// What happened to a refused target that had been created.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalCleanup {
    /// The provider removed it.
    Removed,
    /// It still exists; its owner must remove it.
    Kept,
    /// The removal outcome is not known.
    Unknown,
}

/// A provider's refusal of a target, saved so it is never used later.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Refusal {
    /// What kind of refusal this is.
    pub kind: RefusalKind,
    /// The exact cause, for people. Never matched on.
    pub message: String,
    /// What happened to a target that had been created.
    pub cleanup: RefusalCleanup,
    /// The provider's identifier for the refused target, if one was created.
    #[serde(default)]
    pub target: Option<String>,
    /// What the target was admitted under before it was refused, if anything.
    #[serde(default)]
    pub admission: Option<Admission>,
}
impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

/// Where one workspace's target stands.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum MachineState {
    /// Saved before the provider is asked to create anything. An attempt that
    /// ends without an outcome is finished by asking again with the same key.
    Provisioning {
        /// The key every attempt passes to the provider.
        key: ProvisionKey,
    },
    /// Saved before an existing target is prepared for SDK use.
    Attaching,
    /// The target exists and may be used.
    Ready {
        /// How to reach the target and what it was admitted under.
        descriptor: BackendDescriptor,
    },
    /// The target may never be used. Final.
    Refused {
        /// Why, and what happened to the target.
        refusal: Refusal,
    },
}
impl MachineState {
    /// Whether `next` may follow `self`. States only move forward: a pending
    /// record ends Ready or Refused, and a Ready target can later be refused,
    /// but only under the admission it was made Ready with.
    pub fn allows(&self, next: &MachineState) -> bool {
        match (self, next) {
            (Self::Provisioning { .. } | Self::Attaching, Self::Ready { .. }) => true,
            (Self::Provisioning { .. } | Self::Attaching, Self::Refused { .. }) => true,
            (Self::Ready { descriptor }, Self::Refused { refusal }) => {
                refusal.admission == descriptor.admission
            }
            // A reconnect may refresh what the descriptor reports, never its target.
            (Self::Ready { descriptor }, Self::Ready { descriptor: next }) => {
                descriptor.reference == next.reference && descriptor.ownership == next.ownership
            }
            _ => false,
        }
    }
}

/// The one saved record for a workspace ID.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineRecord {
    /// The caller's ID for the workspace.
    pub workspace_id: WorkspaceId,
    /// Hash of the create or attach request that reserved this ID. `None` for a
    /// target saved by reconnecting to a reference made elsewhere.
    pub request: Option<String>,
    /// Increases by one on every change, for compare-and-swap updates.
    pub revision: u64,
    /// Where the target stands.
    pub state: MachineState,
}
impl MachineRecord {
    /// The descriptor of a Ready target.
    pub fn descriptor(&self) -> Option<&BackendDescriptor> {
        match &self.state {
            MachineState::Ready { descriptor } => Some(descriptor),
            _ => None,
        }
    }
    /// Apply `next` if it is a legal move from the current state.
    pub fn advance(&self, next: MachineState) -> Result<Self> {
        if !self.state.allows(&next) {
            return Err(Error::Conflict(format!(
                "workspace {} cannot move from its saved state to the requested one",
                self.workspace_id
            )));
        }
        if let MachineState::Ready { descriptor } = &next {
            if descriptor.reference.id != self.workspace_id {
                return Err(Error::Conflict(
                    "a machine record cannot hold another workspace's target".into(),
                ));
            }
        }
        Ok(Self {
            workspace_id: self.workspace_id.clone(),
            request: self.request.clone(),
            revision: self.revision + 1,
            state: next,
        })
    }
}
