//! Declarative workload environment and resource intent.
//!
//! An environment is a value within a preparation request, not a provisioned machine
//! or mutable remote environment entity.

use super::{ResourceRequirements, SecretRequirement};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The workload's declarative resource and environment needs.
///
/// Included in the fingerprint of a [`crate::runtime::PreparationRequest`]. Its default has
/// no resource constraints, variables, or secrets. The service checks resources
/// before mutation and separates ordinary values from resolved credential material.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentSpec {
    /// Minimum target resources and exact platform constraints checked against the
    /// host's current [`crate::runtime::AvailableResources`]. This check does not provision them.
    pub resources: ResourceRequirements,
    /// Nonsecret environment values for runtime, probe, and setup commands. Names and
    /// values are validated; private auth directories and reserved credential variables
    /// must be expressed through the dedicated runtime/authentication configuration.
    pub variables: BTreeMap<String, String>,
    /// Credentials supplied to prepared workload launch commands. Setup-only material
    /// belongs in [`crate::runtime::SetupStep::secrets`] so it is not retained in the launch plan.
    pub secrets: Vec<SecretRequirement>,
}
