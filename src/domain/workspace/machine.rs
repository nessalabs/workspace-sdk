//! The saved record of one workspace's target: one place, one set of states.
use super::{BackendDescriptor, Ownership, WorkspaceId};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The provider's idempotency key for creating one workspace's target.
///
/// It is saved in the Provisioning record and derived from the workspace ID and
/// the record's generation, so every attempt to finish one record, including one
/// after a restart, asks the provider for the same target, while a create made
/// after [`crate::WorkspaceSdk::forget_unfinished`] never reuses the key of the
/// forgotten one.
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
    /// The key for a new record of `workspace_id` when `retired` is the last
    /// revision of a forgotten record for it (0 when none was forgotten, which
    /// gives [`Self::for_workspace`], the key older versions used). A record's
    /// key is `after_forgetting(id, record.generation - 1)`.
    pub fn after_forgetting(workspace_id: &WorkspaceId, retired: u64) -> Self {
        let key = Self::for_workspace(workspace_id);
        if retired == 0 {
            return key;
        }
        Self(format!("{}-{retired}", key.0))
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
impl std::fmt::Display for RefusalKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::EnvironmentMismatch => "environment mismatch",
            Self::PendingWipe => "pending wipe",
            Self::NotAdmitted => "not admitted",
            Self::Other => "refused",
        })
    }
}
impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
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
    /// but only under the admission it was made Ready with. A Ready target may
    /// be refreshed, but never changes its target, ownership or admission.
    pub fn allows(&self, next: &MachineState) -> bool {
        match (self, next) {
            (Self::Provisioning { .. } | Self::Attaching, Self::Ready { .. }) => true,
            (Self::Provisioning { .. } | Self::Attaching, Self::Refused { .. }) => true,
            (Self::Ready { descriptor }, Self::Refused { refusal }) => {
                refusal.admission == descriptor.admission
            }
            (Self::Ready { descriptor }, Self::Ready { descriptor: next }) => {
                descriptor.reference == next.reference
                    && descriptor.ownership == next.ownership
                    && descriptor.admission == next.admission
            }
            _ => false,
        }
    }
    /// Whether this state has no outcome yet.
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Provisioning { .. } | Self::Attaching)
    }
    /// The ownership a pending state will end with.
    pub(crate) fn intended_ownership(&self) -> Option<Ownership> {
        match self {
            Self::Provisioning { .. } => Some(Ownership::SdkOwned),
            Self::Attaching => Some(Ownership::Attached),
            Self::Ready { descriptor } => Some(descriptor.ownership.clone()),
            Self::Refused { .. } => None,
        }
    }
}

/// Where a record's ID reservation came from.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "origin")]
pub enum RequestOrigin {
    /// A create or attach request, by the hash of its exact contents.
    Request {
        /// Hash of the request, including whether it was a create or attach.
        hash: String,
    },
    /// Upgraded from a state file written before requests were recorded this
    /// way. The first create or attach for the ID that fits the saved state
    /// adopts it.
    Upgraded,
    /// Saved by reconnecting to a reference made elsewhere; never adopted.
    Connected,
}

/// The one saved record for a workspace ID.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineRecord {
    /// The caller's ID for the workspace.
    pub workspace_id: WorkspaceId,
    /// Where the ID's reservation came from.
    pub origin: RequestOrigin,
    /// Increases on every change and never repeats for an ID, even after the
    /// record is forgotten, so a compare-and-swap cannot land on a newer record.
    pub revision: u64,
    /// The revision this record started at. A record made after a forget has
    /// a higher one, so a call that reserved an earlier record of the same
    /// request can tell it is not its own.
    pub generation: u64,
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
    /// The first revision a new record for an ID may use, given the last
    /// revision of a forgotten record for it.
    pub fn first_revision(retired: Option<u64>) -> u64 {
        retired.map_or(1, |last| last + 1)
    }
    /// A Ready record saved by reconnecting to a reference made elsewhere.
    pub fn connected(descriptor: &BackendDescriptor, retired: Option<u64>) -> Self {
        Self {
            workspace_id: descriptor.reference.id.clone(),
            origin: RequestOrigin::Connected,
            revision: Self::first_revision(retired),
            generation: Self::first_revision(retired),
            state: MachineState::Ready {
                descriptor: descriptor.clone(),
            },
        }
    }
    /// Whether `self`, read later, is still the record `reserved` started:
    /// the same request and the same generation.
    pub fn continues(&self, reserved: &Self) -> bool {
        self.origin == reserved.origin && self.generation == reserved.generation
    }
    /// Whether this record says the SDK itself created its target: a create
    /// finished it, not a reconnect that only reported ownership.
    pub fn created_here(&self) -> bool {
        self.origin != RequestOrigin::Connected
            && self
                .descriptor()
                .is_some_and(|descriptor| descriptor.ownership == Ownership::SdkOwned)
    }
    /// Check a record read from storage, given the last revision forgotten for
    /// its ID.
    pub fn validate(&self, retired: Option<u64>) -> Result<()> {
        if self.generation < Self::first_revision(retired) || self.revision < self.generation {
            return Err(Error::State(
                "machine record generation is not above every forgotten revision".into(),
            ));
        }
        if self
            .descriptor()
            .is_some_and(|descriptor| descriptor.reference.id != self.workspace_id)
        {
            return Err(Error::State(
                "machine record holds another workspace's target".into(),
            ));
        }
        if self.state.is_pending() && self.origin == RequestOrigin::Connected {
            return Err(Error::State(
                "a reconnected record cannot be pending".into(),
            ));
        }
        Ok(())
    }
    /// Reserve `id` for `request`, given the record already saved for it.
    ///
    /// Returns the record to use and whether it must be saved. The same request
    /// continues from its saved record; an upgraded record is adopted by the
    /// first request whose kind fits it; anything else is a conflict.
    /// `retired` is the last revision of a forgotten record for `id`. A new
    /// Provisioning record always gets the key of its generation, whatever key
    /// `initial` carries.
    pub fn begin(
        saved: Option<&Self>,
        id: &WorkspaceId,
        request: &str,
        initial: MachineState,
        retired: Option<u64>,
    ) -> Result<(Self, bool)> {
        let next_revision = Self::first_revision(retired);
        let wanted = initial
            .intended_ownership()
            .filter(|_| initial.is_pending())
            .ok_or_else(|| {
                Error::InvalidInput("a machine record starts Provisioning or Attaching".into())
            })?;
        // A new create's key belongs to its record's generation, so a create
        // made after a forget never reuses a forgotten record's key.
        let initial = match initial {
            MachineState::Provisioning { .. } => MachineState::Provisioning {
                key: ProvisionKey::after_forgetting(id, next_revision - 1),
            },
            other => other,
        };
        let Some(saved) = saved else {
            let record = Self {
                workspace_id: id.clone(),
                origin: RequestOrigin::Request {
                    hash: request.to_owned(),
                },
                revision: next_revision,
                generation: next_revision,
                state: initial,
            };
            return Ok((record, true));
        };
        match &saved.origin {
            RequestOrigin::Request { hash } if hash == request => Ok((saved.clone(), false)),
            RequestOrigin::Request { .. } if saved.state.is_pending() => {
                Err(Error::Conflict(format!(
                    "workspace {id} has an unfinished different request; finish it, or call \
                     forget_unfinished once its provider holds nothing for it"
                )))
            }
            RequestOrigin::Request { .. } => Err(Error::Conflict(format!(
                "workspace {id} already has a different create or attach request"
            ))),
            RequestOrigin::Connected => Err(Error::Conflict(format!(
                "workspace {id} was connected externally; reconnect it by reference"
            ))),
            RequestOrigin::Upgraded => {
                // An upgraded unfinished reservation does not say whether it was
                // a create or an attach, so either may take it over.
                let state = if saved.state.is_pending() {
                    initial
                } else if saved.state.intended_ownership() == Some(wanted) {
                    saved.state.clone()
                } else {
                    return Err(Error::Conflict(format!(
                        "workspace {id} was saved by an older version with a different kind of target"
                    )));
                };
                let record = Self {
                    workspace_id: id.clone(),
                    origin: RequestOrigin::Request {
                        hash: request.to_owned(),
                    },
                    revision: saved.revision + 1,
                    generation: saved.generation,
                    state,
                };
                Ok((record, true))
            }
        }
    }
    /// Apply `next` for a caller that read this record at `expected_revision`,
    /// if it is a legal move from the current state.
    pub fn advance(&self, expected_revision: u64, next: MachineState) -> Result<Self> {
        if self.revision != expected_revision {
            return Err(Error::Conflict(format!(
                "workspace {} changed since it was read",
                self.workspace_id
            )));
        }
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
            origin: self.origin.clone(),
            revision: self.revision + 1,
            generation: self.generation,
            state: next,
        })
    }
    /// Record a descriptor seen by reconnecting. A reconnect cannot see what
    /// the target was admitted under, so the saved admission is kept. Returns
    /// `None` when nothing changed.
    pub fn refresh(&self, descriptor: &BackendDescriptor) -> Result<Option<Self>> {
        let MachineState::Ready { descriptor: saved } = &self.state else {
            return Err(Error::Conflict(format!(
                "workspace {} is not ready; reconnecting cannot finish it",
                self.workspace_id
            )));
        };
        let mut next = descriptor.clone();
        next.admission = saved.admission.clone();
        if *saved == next {
            return Ok(None);
        }
        self.advance(self.revision, MachineState::Ready { descriptor: next })
            .map(Some)
            .map_err(|_| {
                Error::Conflict(format!(
                    "workspace {} cannot be rebound to a different target",
                    self.workspace_id
                ))
            })
    }
    /// Check that this record may be forgotten by a caller that read it at
    /// `expected_revision`. Only a record with no outcome may be forgotten.
    pub fn check_forget(&self, expected_revision: u64) -> Result<()> {
        if self.revision != expected_revision {
            return Err(Error::Conflict(format!(
                "workspace {} changed since it was read",
                self.workspace_id
            )));
        }
        if !self.state.is_pending() {
            return Err(Error::Conflict(format!(
                "workspace {} has an outcome and cannot be forgotten",
                self.workspace_id
            )));
        }
        Ok(())
    }
}
