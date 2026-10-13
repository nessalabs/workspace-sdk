//! Portable file checkpoints, staged reconciliation, and explicit recovery.
//! Blocking operations belong on an async application's blocking executor.

pub mod domain;
pub mod infrastructure;
pub mod ports;

pub use domain::*;
pub use infrastructure::{
    cleanup_transfer, export_remote_tree, import_remote_tree, GitCheckpointEngine,
};
pub use ports::ICheckpointEngine;
