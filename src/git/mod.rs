//! Git observations and explicit managed worktree changes through a workspace.
//!
//! Inventory paths are target-host locators, not workspace identities or ownership
//! claims. Callers compose their own typed provider attachment when appropriate.
mod command;
mod create;
mod discovery;
mod porcelain;
mod remove;
mod validation;
mod worktree;

pub use create::GitWorktreeCreateRequest;
pub use discovery::GitWorktrees;
pub use remove::GitWorktreeRemoveRequest;
/// Backward-compatible name for the same [`GitWorktrees`] connection.
///
/// Existing `new` and `discover` calls retain their behavior. This alias does not
/// construct a separate host binding or introduce another resource identity.
pub type GitWorktreeDiscovery = GitWorktrees;
pub use worktree::{GitWorktree, GitWorktreeHead, GitWorktreeInventory};
