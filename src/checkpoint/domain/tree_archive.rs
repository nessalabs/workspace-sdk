//! Portable file-tree transport values shared with target-side helpers.
//!
//! An archive is a serializable envelope, not a workspace entity or Git history.
//! Receiving engines validate its contents before admitting or materializing them.

use super::FileEntry;
use serde::{Deserialize, Serialize};

/// Versioned JSON envelope used to move selected file bytes through target helpers.
///
/// Unlike a [`super::CheckpointBundle`], this contains no Git objects or history.
/// Each entry carries metadata plus base64 content, allowing the same envelope
/// to travel over ordinary bounded backend file chunks. The receiver validates
/// paths, sizes, hashes, symlink relationships, and mandatory exclusions before
/// restoration or checkpoint capture.
///
/// An archive preserves an already selected tree. Capturing it into a checkpoint
/// does not reapply `.gitignore`, which would otherwise lose files that were
/// tracked-but-ignored in the source repository. Exclusion reports and environment
/// metadata are not part of this envelope.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeArchive {
    /// Envelope format version; only version one is currently supported.
    pub version: u32,
    /// Complete selected file/symlink entries, without separate directory entries.
    ///
    /// Exporters sort by path. Receivers validate duplicate and overlapping paths
    /// and canonicalize the selection before computing its fingerprint.
    pub entries: Vec<TreeArchiveEntry>,
}

/// Content-bearing entry in a [`TreeArchive`] JSON envelope.
///
/// Metadata is flattened into the entry object in JSON, so fields such as `path`
/// and `mode` sit beside `data_base64`. Deserialization alone does not establish
/// trust; engine import checks that decoded bytes match all declared metadata.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeArchiveEntry {
    /// Relative path, content kind, portable mode, decoded size, and SHA-256.
    #[serde(flatten)]
    pub file: FileEntry,
    /// Standard base64 encoding of the complete file bytes or UTF-8 symlink target.
    ///
    /// The decoded length and hash must match [`FileEntry::size`] and
    /// [`FileEntry::sha256`]; encoded and decoded limits are checked before writes.
    pub data_base64: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint::domain::EntryKind;
    use serde_json::Value;

    #[test]
    fn portable_archive_envelope_uses_flattened_entry_metadata() {
        let payload = r#"{"version":1,"entries":[{"path":"binary.bin","kind":"file","mode":100755,"size":1,"sha256":"6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d","data_base64":"AA=="}]}"#;
        let archive: TreeArchive = serde_json::from_str(payload).unwrap();
        assert_eq!(archive.entries[0].file.kind, EntryKind::File);
        assert_eq!(archive.entries[0].file.mode, 100755);
        assert_eq!(
            serde_json::to_value(archive).unwrap(),
            serde_json::from_str::<Value>(payload).unwrap()
        );
    }
}
