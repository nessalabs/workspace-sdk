//! Optional attach-only SPACE adapter for the pinned native command/SSE and
//! incubating file API profile. SPACE sessions transport short supervisor RPCs;
//! existing SDK native receipts remain the authority for runs and output.
mod client;
mod configuration;
mod events;
mod provider;
mod target;
pub use configuration::{SpaceConfig, SpaceOpenOptions};
pub use provider::SpaceProvider;
