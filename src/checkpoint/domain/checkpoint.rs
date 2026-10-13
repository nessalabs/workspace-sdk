//! Immutable checkpoint artifacts, identity values, manifests, and Git bundles.
//!
//! These values describe a fixed selected tree. A new capture produces a new
//! artifact; it does not mutate an existing checkpoint entity.

use super::{ExcludedPath, FileEntry};
use serde::{Deserialize, Serialize};

/// Version of the portable checkpoint manifest understood by this release.
///
/// [`crate::checkpoint::GitCheckpointEngine`] rejects other versions during
/// import instead of interpreting a potentially incompatible snapshot.
pub const CHECKPOINT_FORMAT_VERSION: u32 = 1;

/// Opaque identity of one immutable snapshot in a checkpoint engine's store.
///
/// Persist this value to export, restore, or reconcile the snapshot later.
/// Importing a [`CheckpointBundle`] makes the same identity available in another
/// engine. It identifies selected file contents; it is neither a Git branch nor
/// a workspace, running process, or authentication session.
///
/// The Git engine generates UUID strings and validates references on use:
/// nonempty, at most 80 bytes, and only ASCII letters, digits, or hyphens.
/// Constructing or deserializing this transparent wrapper alone does not perform
/// that validation. Use the [`CheckpointManifest::tree_fingerprint`] to compare
/// contents, since distinct identities can contain identical trees.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CheckpointRef(
    /// Store identifier serialized as a plain JSON string, never a filesystem path.
    pub String,
);

/// Domain value describing one immutable, selected file artifact.
///
/// This is not a mutable entity: an engine-produced checkpoint describes a fixed
/// selection, and another capture creates another artifact. Publicly constructed
/// values become trusted only after the receiving engine validates them.
///
/// A checkpoint captures project files, executable bits, and supported symlinks;
/// it does not capture installed tools, process memory, or credentials. Its bytes
/// remain in the injected engine's private store until explicitly exported.
/// Capturing never changes the source repository's branch or index.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Versioned selection, ancestry metadata, and integrity information for this snapshot.
    pub manifest: CheckpointManifest,
}

impl Checkpoint {
    /// Borrow the durable identity used by checkpoint, sync, and handoff operations.
    ///
    /// This does not load or copy file contents. The engine's private state must
    /// remain available, or the corresponding bundle must be imported elsewhere.
    pub fn reference(&self) -> &CheckpointRef {
        &self.manifest.reference
    }
}

/// Versioned description and integrity record accompanying a checkpoint bundle.
///
/// The Git engine validates this metadata and the actual Git objects on import.
/// Selected entries are sorted by relative path, and their hashes, sizes, modes,
/// and aggregate fingerprint must agree with the bundle. Public fields support
/// transport and persistence; assembling or deserializing a manifest does not
/// itself validate these invariants.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckpointManifest {
    /// Manifest schema version; currently [`CHECKPOINT_FORMAT_VERSION`].
    pub version: u32,
    /// Stable identity assigned to this selected snapshot.
    pub reference: CheckpointRef,
    /// Optional logical ancestor used by the application; its history is not bundled.
    ///
    /// This is metadata, not an instruction to fetch or validate the parent.
    pub parent: Option<CheckpointRef>,
    /// Optional non-secret identifier for the environment/setup recipe to recreate.
    ///
    /// The checkpoint engine retains this string without installing or resolving it.
    pub environment_reference: Option<String>,
    /// Forty-character SHA-1 Git object ID of the standalone snapshot commit.
    ///
    /// Imported commits must have no Git parents, so excluded historical data
    /// cannot travel through the user's source repository history.
    pub commit: String,
    /// Lowercase SHA-256 of the canonical JSON serialization of sorted [`FileEntry`] values.
    ///
    /// This fingerprints selected bytes, kinds, paths, and executable modes, not
    /// timestamps, excluded files, credentials, or the complete filesystem.
    pub tree_fingerprint: String,
    /// Lowercase SHA-256 of the complete serialized Git bundle bytes.
    pub bundle_sha256: String,
    /// Exact Git bundle length in bytes, checked against transport limits.
    pub bundle_bytes: u64,
    /// Sum of decoded content sizes across all selected entries, in bytes.
    ///
    /// Symlink targets contribute their UTF-8 byte length to this total.
    pub total_bytes: u64,
    /// Complete selected file tree, sorted lexicographically by relative path.
    ///
    /// Empty directories and unsupported filesystem metadata are not represented.
    pub entries: Vec<FileEntry>,
    /// Paths omitted during local capture and the policy responsible for each omission.
    ///
    /// A directory entry can represent an excluded subtree. Imported tree archives
    /// do not carry this report, so an empty vector does not establish that the
    /// source had no excluded files.
    pub excluded: Vec<ExcludedPath>,
}

/// Self-contained, portable Git snapshot together with its validation manifest.
///
/// Export this value from one engine and import it into another to preserve the
/// checkpoint identity. Bundles contain only the selected standalone snapshot,
/// never the source repository's branch history. Import checks size limits,
/// compressed-object bounds, hashes, entry metadata, and forbidden paths before
/// admitting it into the private checkpoint store.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckpointBundle {
    /// Expected identity, selected tree, byte lengths, and checksums.
    pub manifest: CheckpointManifest,
    /// Raw Git bundle bytes whose hash and length must match `manifest`.
    ///
    /// This is a Git bundle, not a tar archive or a [`super::TreeArchive`] JSON envelope.
    pub bytes: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_wire_format_remains_an_opaque_string_for_saved_bindings() {
        let reference: CheckpointRef = serde_json::from_str("\"checkpoint-123\"").unwrap();
        assert_eq!(reference.0, "checkpoint-123");
        assert_eq!(
            serde_json::to_string(&reference).unwrap(),
            "\"checkpoint-123\""
        );
    }
}
