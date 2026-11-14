//! Agent37 Cloud (<https://www.agent37.com>): persistent Linux instances
//! made from pinned templates, reached over an authenticated SSH tunnel.
//!
//! [`Agent37Machines`] makes, admits, wakes, stops and deletes a caller's
//! instances; [`Agent37Provider`] attaches a ready one as a workspace and
//! opens live connections into it.
mod api;
mod config;
mod failure;
mod identity;
mod machines;
mod provider;
mod spec;
mod target;
mod tunnel;

pub use config::{Agent37Config, ApiKey, DEFAULT_API, DEFAULT_TUNNEL};
pub use failure::{AdmissionRefusal, Agent37Failure};
pub use identity::Agent37Identity;
pub use machines::{Admission, Agent37Machines, Created};
pub use provider::{Agent37AttachOptions, Agent37Provider};
pub use spec::{MachineSpec, Size, IDLE};

#[cfg(test)]
mod live_tests;
