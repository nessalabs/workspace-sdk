//! Git-backed checkpoint storage and native, bounded target-side tree transport.
//!
//! [`GitCheckpointEngine`] owns private snapshot/reconciliation state outside
//! projects. The transfer functions publish or consume the same archive format
//! inside a target's scoped staging directory without importing Git history.

mod apply;
mod archive;
mod bundle;
mod capture;
mod checkpoint_engine_adapter;
mod file_tree;
mod filesystem;
mod git_checkpoint_engine;
mod git_command;
mod inspect_apply;
mod pack;
mod path_policy;
mod reconciliation;
mod remote_import;
mod remote_storage;
mod remote_tree;
mod scanner;
mod snapshot;
mod storage;

pub use git_checkpoint_engine::GitCheckpointEngine;
pub use remote_tree::{cleanup_transfer, export_remote_tree, import_remote_tree};
