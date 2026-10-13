//! File-selection values: portable metadata, content kinds, and exclusion reports.
//!
//! A file entry describes one path in one snapshot, not a mutable filesystem
//! entity or a globally stable file identity.

use serde::{Deserialize, Serialize};

/// Filesystem content kinds represented in a portable checkpoint.
///
/// Directories are inferred from entry paths. Sockets, devices, FIFOs, and other
/// unsupported filesystem features are not represented as ordinary file data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// Regular file contents, including arbitrary binary bytes.
    File,
    /// A relative symlink whose stored bytes are its UTF-8 target text.
    ///
    /// The engine validates both individual targets and the complete symlink
    /// graph so they cannot escape the snapshot or enter excluded control/secret paths.
    Symlink,
}

/// Portable metadata for one selected file or symlink.
///
/// The entry's content bytes live in a Git object or [`super::TreeArchiveEntry`].
/// Together with its relative path and mode, the checksum contributes to the
/// snapshot fingerprint. The format intentionally omits timestamps, ownership,
/// ACLs, extended attributes, and empty directories.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// UTF-8 path relative to the project root, using `/` separators.
    ///
    /// Validation rejects absolute/parent paths, control characters, backslashes,
    /// drive-like colons, excluded paths, duplicate entries, ancestor overlaps,
    /// and case-colliding path components in the selected tree.
    pub path: String,
    /// Whether the content bytes are a regular file or a symlink target.
    pub kind: EntryKind,
    /// Numeric spelling of the Git mode: `100644`, `100755`, or `120000`.
    ///
    /// These are format values, not Rust octal permission literals. Regular files
    /// use the first two modes; symlinks must use `120000`.
    pub mode: u32,
    /// Decoded content length in bytes; symlinks count the UTF-8 target text.
    pub size: u64,
    /// Lowercase, 64-character SHA-256 of the decoded content bytes.
    pub sha256: String,
}

/// Explanation for a path omitted from a local checkpoint capture.
///
/// Mandatory secret/control/cache exclusions apply even to tracked files and
/// cannot be negated by ignore rules. Untracked files also obey `.gitignore`,
/// while `.workspaceignore` can exclude additional tracked or untracked content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExcludedPath {
    /// Project-relative path of the omitted file or directory.
    ///
    /// A directory represents its entire skipped subtree; descendants need not
    /// have separate records.
    pub path: String,
    /// Human-readable policy explanation, such as `.gitignore` or a mandatory exclusion.
    ///
    /// This is descriptive text rather than a stable machine-readable reason code.
    pub reason: String,
}
