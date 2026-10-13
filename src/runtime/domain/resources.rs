//! Resource predicates and host observations used before runtime mutation.
//!
//! Requirements state minima and exact platform matches. Available values report a
//! point-in-time capacity snapshot; neither kind reserves or allocates compute.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// A snapshot of one GPU visible to the runtime host.
///
/// The resource check matches each GPU independently against a requested model and
/// per-device memory minimum; this value neither allocates nor reserves a device.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailableGpu {
    /// Observed model name. Matching is exact and case-sensitive, without aliases.
    pub model: String,
    /// Observed total device memory in mebibytes, not currently unused GPU memory.
    pub memory_mib: u64,
}

/// A snapshot of resources visible to the injected runtime host.
///
/// This is an observation, not a reservation or a provisioning request. Values may
/// change after validation. [`crate::runtime::ResourceRequirements`] treats unknown numeric
/// values as insufficient when a corresponding minimum is requested.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailableResources {
    /// Host-reported operating-system name, compared exactly to any requested name.
    /// The backend adapter currently reports Python platform names such as `linux`.
    pub os: String,
    /// Host-reported machine architecture, compared exactly without alias conversion.
    pub architecture: String,
    /// Observed usable logical CPU count, when known. The backend adapter accounts
    /// for affinity and supported container quota limits.
    pub cpus: Option<u32>,
    /// Observed memory capacity in mebibytes (1 MiB = 1,048,576 bytes), when known.
    /// This is capacity subject to detected limits, not a promise of currently free RAM.
    pub memory_mib: Option<u64>,
    /// Observed free disk space in mebibytes at the host adapter's inspected filesystem.
    pub disk_mib: Option<u64>,
    /// Individually observed GPUs. An empty list provides no evidence of a usable GPU;
    /// each matching entry counts once toward a [`crate::runtime::GpuRequirement`].
    pub gpus: Vec<AvailableGpu>,
}

/// The minimum number of GPUs that must each satisfy optional device constraints.
///
/// Used within [`crate::runtime::ResourceRequirements`]. The count is taken from matching
/// [`crate::runtime::AvailableGpu`] observations; memory is per device rather than pooled.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuRequirement {
    /// Required matching device count. Zero is invalid.
    pub count: u32,
    /// Optional minimum total memory per matching GPU, in mebibytes.
    pub min_memory_mib: Option<u64>,
    /// Optional exact, case-sensitive device model name; `None` accepts any model.
    pub model: Option<String>,
}

/// Value-object constraints checked before runtime installation or setup.
///
/// Every populated constraint must match the host's [`crate::runtime::AvailableResources`].
/// Defaults impose no constraints. These are validation requirements, not provider
/// provisioning settings; applications may also validate proposed compute beforehand.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRequirements {
    /// Required exact host operating-system name, or no OS restriction.
    pub os: Option<String>,
    /// Required exact architecture string, or no architecture restriction.
    pub architecture: Option<String>,
    /// Minimum logical CPU count. A requested minimum must be positive and known
    /// availability must meet it.
    pub min_cpus: Option<u32>,
    /// Positive minimum memory capacity in mebibytes, or no memory constraint.
    pub min_memory_mib: Option<u64>,
    /// Positive minimum free disk space in mebibytes, or no disk constraint.
    pub min_disk_mib: Option<u64>,
    /// Optional GPU count and per-device constraints.
    pub gpu: Option<GpuRequirement>,
}

impl ResourceRequirements {
    /// Check all populated constraints against one resource observation.
    ///
    /// Unknown CPU, memory, or disk availability cannot satisfy a requested minimum.
    /// OS, architecture, and GPU model strings match exactly. Each GPU must independently
    /// meet the requested model and memory constraints to count.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] for zero CPU/memory/disk minimums or a zero
    /// GPU count, and [`crate::Error::Unsupported`] when the observed target does not
    /// satisfy the requirements. This method has no provisioning side effects.
    pub fn validate_available(&self, available: &AvailableResources) -> Result<()> {
        if self.min_cpus == Some(0)
            || self.min_memory_mib == Some(0)
            || self.min_disk_mib == Some(0)
            || self.gpu.as_ref().is_some_and(|gpu| gpu.count == 0)
        {
            return Err(Error::InvalidInput(
                "resource minimums must be positive".into(),
            ));
        }
        let unsupported = self.os.as_ref().is_some_and(|os| os != &available.os)
            || self
                .architecture
                .as_ref()
                .is_some_and(|arch| arch != &available.architecture)
            || self
                .min_cpus
                .is_some_and(|min| available.cpus.is_none_or(|value| value < min))
            || self
                .min_memory_mib
                .is_some_and(|min| available.memory_mib.is_none_or(|value| value < min))
            || self
                .min_disk_mib
                .is_some_and(|min| available.disk_mib.is_none_or(|value| value < min))
            || self.gpu.as_ref().is_some_and(|gpu| {
                available
                    .gpus
                    .iter()
                    .filter(|actual| {
                        gpu.model
                            .as_ref()
                            .is_none_or(|model| model == &actual.model)
                            && gpu
                                .min_memory_mib
                                .is_none_or(|min| actual.memory_mib >= min)
                    })
                    .count()
                    < gpu.count as usize
            });
        if unsupported {
            return Err(Error::Unsupported(
                "target does not satisfy runtime resource requirements".into(),
            ));
        }
        Ok(())
    }
}
