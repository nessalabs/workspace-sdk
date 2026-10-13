//! Capture request values and the synchronization context's resource-limit policy.
//!
//! These are configuration values consumed by an engine, not entities with an
//! independent lifecycle. They carry no credentials or provisioning behavior.

use super::CheckpointRef;
use serde::{Deserialize, Serialize};

/// Optional application metadata attached to a newly captured snapshot.
///
/// Defaults leave both fields absent. These values neither change file-selection
/// policy nor transfer environments or authentication material. Ordinary capture
/// still filters secrets/control state and checks for changes during the read;
/// the caller remains responsible for stopping writers at a safe boundary.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CaptureOptions {
    /// Logical ancestor of the new snapshot, when the application knows one.
    ///
    /// Private Git commits remain standalone: setting this does not include,
    /// fetch, or merge the ancestor's files or repository history.
    pub parent: Option<CheckpointRef>,
    /// Opaque, non-secret reference to the setup/environment needed after restoration.
    ///
    /// The engine records it unchanged; the application recreates that environment.
    pub environment_reference: Option<String>,
}

/// Resource limits applied by the configured local Git checkpoint engine.
///
/// All values must be positive when supplied to
/// [`crate::checkpoint::GitCheckpointEngine::with_limits`]. Limits bound selected
/// content, manifests, bundles, and archive decoding. The JSON tree envelope's
/// encoded limit is derived from base64 expansion plus the metadata allowance.
/// Native target-side transfers use their own fixed default bounds; customizing
/// these receiver-side limits does not reconfigure the target supervisor.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckpointLimits {
    /// Maximum selected file/symlink entries; the default is 25,000.
    pub max_files: usize,
    /// Maximum decoded bytes in one file or symlink target; the default is 32 MiB.
    pub max_file_bytes: u64,
    /// Maximum aggregate decoded content bytes; the default is 256 MiB.
    pub max_total_bytes: u64,
    /// Maximum raw Git bundle bytes accepted or exported; the default is 300 MiB.
    pub max_bundle_bytes: u64,
    /// Maximum bytes for a serialized manifest or engine metadata record.
    ///
    /// The default is 16 MiB. This allowance also contributes to encoded archive
    /// bounds and to the allowance for non-file objects during bundle validation.
    pub max_manifest_bytes: u64,
}

impl Default for CheckpointLimits {
    fn default() -> Self {
        Self {
            max_files: 25_000,
            max_file_bytes: 32 * 1024 * 1024,
            max_total_bytes: 256 * 1024 * 1024,
            max_bundle_bytes: 300 * 1024 * 1024,
            max_manifest_bytes: 16 * 1024 * 1024,
        }
    }
}
