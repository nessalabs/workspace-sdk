//! Bounded live observations. Revision tokens detect staleness, not a writer lock.
use super::WorkspacePath;
use serde::{Deserialize, Serialize};

/// Opaque workspace/path metadata observation token. Keep it on the same connected
/// target; it is neither authorization nor a content-addressed snapshot. Detection
/// depends on filesystem inode/timestamp semantics. It does not fence writers.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileRevision(pub String);

/// A directory entry. Symlinks and special files are visible but cannot be read.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DirectoryEntry {
    /// Normalized project-relative path.
    pub path: WorkspacePath,
    /// Observed type without following symlinks.
    pub kind: FileKind,
    /// Metadata length in bytes; meaningful as content size for regular files.
    pub size: u64,
    /// Opaque token tied to this path and observation.
    pub revision: FileRevision,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
/// Native filesystem entry classification.
pub enum FileKind {
    /// Regular binary file.
    File,
    /// Directory, including an empty directory.
    Directory,
    /// Symbolic link; the browser never follows it.
    Symlink,
    /// Special entry such as a FIFO or socket.
    Other,
}
/// Continuation must be reused with the same workspace and directory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DirectoryCursor {
    /// Opaque token tied to this path and observation.
    pub revision: FileRevision,
    /// Number of entries already delivered.
    pub offset: u32,
}
/// Native targets scan at most 4096 entries and return at most 256 per page.
/// A changed continuation returns Conflict; discard pages and restart at offset 0.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListDirectory {
    /// Normalized project-relative path.
    pub path: WorkspacePath,
    /// Maximum entries or bytes returned, bounded by the operation contract.
    pub limit: u32,
    /// Prior continuation, or None to start a fresh observation.
    pub cursor: Option<DirectoryCursor>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
/// One bounded page from a directory observation.
pub struct DirectoryPage {
    /// Immediate children ordered by normalized path.
    pub entries: Vec<DirectoryEntry>,
    /// Opaque token tied to this path and observation.
    pub revision: FileRevision,
    /// Continuation for the same observation; None at the end.
    pub next: Option<DirectoryCursor>,
}
/// Read at most 256 KiB. A supplied revision must match before and after reading.
/// Retain the first result's revision for every subsequent chunk. Restart on Conflict.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadVersionedFile {
    /// Normalized project-relative path.
    pub path: WorkspacePath,
    /// Zero-based starting byte offset.
    pub offset: u64,
    /// Maximum entries or bytes returned, bounded by the operation contract.
    pub limit: u32,
    /// Required revision, or None to establish an initial token.
    pub expected: Option<FileRevision>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
/// Bytes read under one checked file revision.
pub struct VersionedFileChunk {
    /// Unencoded binary bytes.
    pub bytes: Vec<u8>,
    /// Starting offset plus returned byte count.
    pub next_offset: u64,
    /// Whether the observed file ends at this chunk.
    pub eof: bool,
    /// Opaque token tied to this path and observation.
    pub revision: FileRevision,
}
/// Publication precondition. Missing means create exclusively; Revision means replace.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FilePrecondition {
    /// Require that no destination entry exists.
    Missing,
    /// Require a matching revision and fenced publication; native targets refuse.
    Revision(FileRevision),
}
/// Native targets support only Missing writes, bounded to 256 KiB, with an existing
/// parent and Linux anonymous-file support. They never fall back to hash-then-replace for Revision. Use checkpoint
/// reconciliation and explicit quiescent apply for existing-file changes.
#[derive(Clone, Serialize, Deserialize)]
pub struct PublishFile {
    /// Normalized project-relative path.
    pub path: WorkspacePath,
    /// Unencoded binary bytes.
    pub bytes: Vec<u8>,
    /// Condition the target must enforce before publication.
    pub expected: FilePrecondition,
    /// Set executable permission on the new file.
    pub executable: bool,
}
/// Unknown publication must be inspected before retry. Conflict never replaces bytes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PublishOutcome {
    /// A complete new file was published without replacing any entry.
    Created {
        /// Revision immediately after publication; later edits can invalidate it.
        revision: FileRevision,
    },
    /// Destination already exists; its content was preserved.
    Conflict,
    /// Publication may have occurred. Inspect before any retry.
    Unknown,
}

impl ListDirectory {
    pub(crate) fn validate(&self) -> crate::Result<()> {
        validate_browser_path(&self.path)?;
        if !(1..=256).contains(&self.limit)
            || self
                .cursor
                .as_ref()
                .is_some_and(|cursor| cursor.offset > 4096 || !valid_revision(&cursor.revision))
        {
            return Err(crate::Error::InvalidInput(
                "invalid directory bounds or cursor".into(),
            ));
        }
        Ok(())
    }
}
impl DirectoryPage {
    pub(crate) fn validate_for(&self, request: &ListDirectory) -> crate::Result<()> {
        let offset = request.cursor.as_ref().map_or(0, |cursor| cursor.offset);
        let valid_next = self.next.as_ref().is_none_or(|next| {
            !self.entries.is_empty()
                && next.revision == self.revision
                && next.offset == offset + self.entries.len() as u32
                && next.offset < 4096
        });
        let parent = if request.path == WorkspacePath::root() {
            ""
        } else {
            request.path.as_str()
        };
        let children = self.entries.iter().all(|entry| {
            std::path::Path::new(entry.path.as_str())
                .parent()
                .and_then(|value| value.to_str())
                == Some(parent)
                && entry.path != WorkspacePath::root()
                && entry.path.as_str().len() <= 4096
                && valid_revision(&entry.revision)
        });
        let sorted = self
            .entries
            .windows(2)
            .all(|pair| pair[0].path.as_str() < pair[1].path.as_str());
        if self.entries.len() > request.limit as usize
            || !valid_revision(&self.revision)
            || !valid_next
            || !children
            || !sorted
            || request
                .cursor
                .as_ref()
                .is_some_and(|cursor| cursor.revision != self.revision)
        {
            return Err(crate::Error::State("invalid directory response".into()));
        }
        Ok(())
    }
}
impl ReadVersionedFile {
    pub(crate) fn validate(&self) -> crate::Result<()> {
        validate_browser_path(&self.path)?;
        if !(1..=262144).contains(&self.limit)
            || self
                .expected
                .as_ref()
                .is_some_and(|value| !valid_revision(value))
        {
            return Err(crate::Error::InvalidInput(
                "invalid versioned read bounds or revision".into(),
            ));
        }
        Ok(())
    }
}
impl PublishFile {
    pub(crate) fn validate(&self) -> crate::Result<()> {
        validate_browser_path(&self.path)?;
        if self.path == WorkspacePath::root()
            || self.bytes.len() > 262144
            || matches!(&self.expected, FilePrecondition::Revision(value) if !valid_revision(value))
        {
            return Err(crate::Error::InvalidInput(
                "invalid publication path, bounds or revision".into(),
            ));
        }
        Ok(())
    }
}
pub(crate) fn valid_revision(value: &FileRevision) -> bool {
    value.0.len() == 64 && value.0.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn validate_browser_path(path: &WorkspacePath) -> crate::Result<()> {
    if path.as_str().len() > 4096 {
        return Err(crate::Error::InvalidInput(
            "browser path exceeds 4096 bytes".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unbounded_revisions_and_root_publication() {
        let read = ReadVersionedFile {
            path: WorkspacePath::new("file").unwrap(),
            offset: 0,
            limit: 1,
            expected: Some(FileRevision("x".repeat(65536))),
        };
        assert!(read.validate().is_err());
        let publish = PublishFile {
            path: WorkspacePath::root(),
            bytes: vec![],
            expected: FilePrecondition::Missing,
            executable: false,
        };
        assert!(publish.validate().is_err());
    }
    #[test]
    fn rejects_non_advancing_and_wrong_target_directory_cursors() {
        let revision = FileRevision("a".repeat(64));
        let request = ListDirectory {
            path: WorkspacePath::root(),
            limit: 1,
            cursor: Some(DirectoryCursor {
                revision: revision.clone(),
                offset: 1,
            }),
        };
        let page = DirectoryPage {
            entries: vec![],
            revision: revision.clone(),
            next: Some(DirectoryCursor {
                revision,
                offset: 2,
            }),
        };
        assert!(page.validate_for(&request).is_err());
    }
}
