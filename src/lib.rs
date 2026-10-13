#![doc = include_str!("../README.md")]
pub mod application;
pub mod checkpoint;
pub mod domain;
mod error;
pub mod infrastructure;
pub mod ports;
pub mod providers;
pub mod runtime;
pub mod supervisor;
pub use application::*;
pub use domain::*;
pub use error::{Error, Result};
pub use ports::{IBackend, IProvider, IWorkspaceRepository};
/// Configure the pinned Boat client without a separate sibling dependency.
#[cfg(feature = "boat")]
pub use sandbox_client as boat_client;
