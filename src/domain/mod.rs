//! Provider-independent domain concepts, grouped by bounded context.
//!
//! - [`workspace`]: target identity, access, resources, files, and lifecycle.
//! - [`execution`]: process commands, durable run identity, and output.
//! - [`synchronization`]: sync relationships and resumable handoff progress.
//!
//! Immutable file checkpoints and review/apply values live in [`crate::checkpoint::domain`];
//! workload setup and authentication live in [`crate::runtime::domain`]. Each
//! context owns its rules and communicates through typed values and interfaces.
//! Root reexports are conveniences; the context modules explain ownership.

#![deny(missing_docs)]

pub mod execution;
mod identifier;
pub mod synchronization;
pub mod workspace;

pub use execution::*;
pub use synchronization::*;
pub use workspace::*;
