//! Read-only Git observations made through an existing workspace execution port.
//!
//! Inventory paths are target-host locators, not workspace identities or ownership
//! claims. Callers compose their own typed provider attachment when appropriate.
mod discovery;
mod porcelain;
mod worktree;

pub use discovery::GitWorktreeDiscovery;
pub use worktree::{GitWorktree, GitWorktreeHead, GitWorktreeInventory};
