//! Native POSIX workspace supervisor and its typed process-transport protocol.
mod cli;
mod dispatcher;
pub(crate) mod filesystem;
pub mod protocol;
mod run_record;
mod terminal;
mod terminal_command;
mod terminal_control;
mod workload;
pub use cli::run_cli;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod directory_entries;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod file_browser;
