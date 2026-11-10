//! Portable file checkpoints, staged reconciliation, and explicit recovery.
//! Blocking operations belong on an async application's blocking executor.
//!
//! The whole [`domain`] module is compiled in every build on purpose. Saved
//! workflow records refer to some of its values (checkpoint and reconciliation
//! references, apply receipts), so a state file reads the same with or without
//! the `portability` feature; the rest is kept with them so the module is not
//! split by feature. The engine (`infrastructure`, `ports`) needs `portability`.

pub mod domain;
#[cfg(feature = "portability")]
pub mod infrastructure;
#[cfg(feature = "portability")]
pub mod ports;

pub use domain::*;
#[cfg(feature = "portability")]
pub use infrastructure::{
    cleanup_transfer, export_remote_tree, import_remote_tree, GitCheckpointEngine,
};
#[cfg(feature = "portability")]
pub use ports::ICheckpointEngine;
