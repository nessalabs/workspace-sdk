//! Portable file checkpoints, staged reconciliation, and explicit recovery.
//! Blocking operations belong on an async application's blocking executor.
//!
//! The [`domain`] values are compiled in every build because saved workflow
//! records refer to them, so a state file reads the same with or without the
//! `portability` feature. The engine (`infrastructure`, `ports`) needs that
//! feature.

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
