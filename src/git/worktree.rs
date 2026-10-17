use crate::WorkspaceRef;
use serde::{Deserialize, Serialize};

/// A point-in-time Git inventory observed from an identity-bearing workspace.
///
/// These serializable values confer no authorization or resource ownership.
/// Paths and branch state may change immediately after discovery. Deserialization
/// reconstructs data; it does not validate a fresh Git observation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GitWorktreeInventory {
    /// Workspace through which Git was executed, including its provider locator.
    pub anchor: WorkspaceRef,
    /// Complete accepted inventory, in Git's reported order; never a partial result.
    pub worktrees: Vec<GitWorktree>,
}

/// One target-host worktree locator and its observed Git metadata.
///
/// Discovery accepts UTF-8 paths up to 16 KiB and rejects replacement characters
/// because the execution port exposes lossy strings. A path does not grant access,
/// create a workspace identity, or guarantee that the directory still exists.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GitWorktree {
    /// Unquoted target-host path, preserving embedded spaces and newlines.
    pub path: String,
    /// Bare repository, attached branch, or detached commit observed by Git.
    pub head: GitWorktreeHead,
    /// Lock reason, at most 4 KiB; `Some("")` means locked without a reason.
    pub locked: Option<String>,
    /// Prune reason, at most 4 KiB; `Some("")` means prunable without a reason.
    pub prunable: Option<String>,
}

/// Mutually exclusive HEAD states reported by Git worktree porcelain.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum GitWorktreeHead {
    /// Bare repository with no worktree HEAD fields.
    Bare,
    /// A local branch, including an unborn branch with an all-zero object ID.
    Branch {
        /// Full `refs/heads/` reference, bounded to 4 KiB during discovery.
        reference: String,
        /// Full 40- or 64-character hexadecimal object ID.
        oid: String,
    },
    /// A detached HEAD with no attached branch.
    Detached {
        /// Full, nonzero 40- or 64-character hexadecimal object ID.
        oid: String,
    },
}
