//! Boat VMs as a caller's machines: made from a [`MachineSpec`], admitted
//! only in one credential posture, resumed, stopped and rolled back.
//!
//! Every Boat answer that grants anything is read strictly (`admission`): the
//! account's environment, the creation acknowledgement and the VM's full
//! info receipt. Nothing here names an application: the spec carries the
//! image, environment, lifetime and variables, and events go to the caller's
//! [`IMachineEvents`] sink, so it can keep its own audit.
//!
//! ```text
//! create:  environment ─► POST /sandboxes (key) ─► wait for start ─► check
//! ready:   check ─► wait out archiving ─► resume ─► wait ready ─► check
//! stop:    StopRequested ─► stop ─► StopAnswered
//! ```
mod account;
mod admission;
mod diagnostics;
mod environment;
mod events;
mod spec;

pub use account::{BoatMachines, Created, Disposition};
pub use diagnostics::{Diagnostic, Stage};
pub use environment::{
    AdmissionRefusal, AdmissionRejection, EnvironmentIdentity, ObservedEnvironment,
};
pub use events::{IMachineEvents, MachineEvent, StopCause};
pub use spec::{
    Budget, MachineFailure, MachineSpec, MAX_BUDGET, MAX_VARIABLES, MAX_VARIABLE_VALUE,
};
