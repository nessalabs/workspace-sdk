//! Resource observations supplied by a target, independent of runtime policy.
use serde::{Deserialize, Serialize};

/// A resource snapshot from the machine hosting a workspace.
///
/// This is an observation, not a reservation. Runtime preparation translates
/// this value into its own resource model and checks workload requirements.
/// An absent optional capacity means it could not be established, not zero.
/// `Default` is an unpopulated observation and does not probe the host.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostResources {
    /// Operating-system name reported by the target resource probe.
    pub os: String,
    /// Machine architecture, such as `x86_64` or `aarch64`.
    pub architecture: String,
    /// Usable logical CPU count when the target can determine it.
    pub cpus: Option<u32>,
    /// Memory capacity subject to detected limits, in mebibytes.
    pub memory_mib: Option<u64>,
    /// Available space on the project filesystem, in mebibytes.
    pub disk_mib: Option<u64>,
    /// GPUs discovered by the target's probe.
    /// An empty list can mean no devices or no usable probe; it does not establish absence.
    pub gpus: Vec<HostGpu>,
}

/// An observed GPU device; this value does not allocate the device.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostGpu {
    /// Device model reported by the target driver.
    pub model: String,
    /// Total device memory in mebibytes.
    pub memory_mib: u64,
}
