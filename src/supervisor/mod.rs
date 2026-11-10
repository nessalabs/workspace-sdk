//! Native POSIX workspace supervisor and its typed process-transport protocol.
mod cli;
mod control_channel;
mod dispatcher;
pub(crate) mod filesystem;
pub mod protocol;
mod run_record;
mod terminal;
mod terminal_command;
mod terminal_control;
mod workload;
pub use cli::run_cli;

#[cfg(all(feature = "portability", any(target_os = "linux", target_os = "macos")))]
mod directory_entries;
#[cfg(all(feature = "portability", any(target_os = "linux", target_os = "macos")))]
mod file_browser;
