//! Binary file-transfer commands and returned chunks.
use super::WorkspacePath;

/// A bounded binary read from a workspace-relative file.
///
/// Read in chunks to avoid loading a large file into a single transport message.
/// The backend must enforce workspace boundaries, including symlink resolution.
#[derive(Clone, Debug)]
pub struct ReadFile {
    /// File path relative to the project root.
    pub path: WorkspacePath,
    /// Zero-based byte offset at which to start reading.
    pub offset: u64,
    /// Maximum number of bytes requested in this chunk.
    pub limit: u32,
}

/// One binary chunk returned by a workspace file read.
///
/// Use `next_offset` for the next [`ReadFile`] request and stop when `eof` is true.
#[derive(Clone, Debug)]
pub struct FileChunk {
    /// Unencoded file bytes in this chunk.
    pub bytes: Vec<u8>,
    /// Absolute byte offset at which the next read should begin.
    pub next_offset: u64,
    /// Whether this read reached the end of the file.
    pub eof: bool,
}

/// One binary write into a workspace-relative file.
///
/// For a fresh upload, truncate on the first chunk and use successive byte
/// offsets for later chunks. A sequence of writes is not an atomic file-sync
/// transaction; use the checkpoint workflow when conflict handling is required.
#[derive(Clone, Debug)]
pub struct WriteFile {
    /// Destination path relative to the project root.
    pub path: WorkspacePath,
    /// Zero-based byte offset at which to write this chunk.
    pub offset: u64,
    /// Unencoded bytes to write.
    pub bytes: Vec<u8>,
    /// Whether to truncate the destination before this write.
    pub truncate: bool,
    /// Whether the resulting file should have executable permission.
    pub executable: bool,
}
