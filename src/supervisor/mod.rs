//! Native POSIX workspace supervisor and its typed process-transport protocol.
mod cli;
mod control_channel;
mod dispatcher;
mod filesystem;
mod pipe_control;
pub mod protocol;
mod run_record;
mod terminal;
mod terminal_command;
mod terminal_control;
mod workload;
pub use cli::run_cli;
