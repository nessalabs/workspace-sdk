//! Native POSIX workspace supervisor and its typed process-transport protocol.
mod cli;
mod dispatcher;
mod filesystem;
pub mod protocol;
mod run_record;
mod workload;
pub use cli::run_cli;
