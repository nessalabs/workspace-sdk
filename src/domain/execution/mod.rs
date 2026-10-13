//! Execution context: commands, managed-run identities, and process observations.
//!
//! A [`crate::Run`] is an identity-bearing application handle. Its [`RunId`]
//! stays constant while the observed [`RunState`] changes. [`RunRef`] adds the
//! workspace needed to reconnect; it is a value object, not another process.
//! [`CommandSpec`] and [`LaunchRequest`] are commands; [`ExecResult`] and
//! [`OutputPage`] are results. None of these values represent an agent’s
//! conversation memory or make a running process portable between machines.

pub mod command;
pub mod output;
pub mod run;

pub use command::{CommandSpec, ExecResult, ExecutionRequest, SecretEnvironment, SecretInput};
pub use output::{OutputCursor, OutputPage};
pub use run::{LaunchRequest, RunId, RunRef, RunState};
