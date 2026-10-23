use crate::{
    checkpoint::{
        CaptureOptions, Checkpoint, EntryKind, FileEntry, ICheckpointEngine, TreeArchive,
        TreeArchiveEntry,
    },
    EditChange, Error, Result, WorkspacePath,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};

pub(super) const MAX_FILES: usize = 4096;
pub(super) const MAX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ARCHIVE: usize = 32 * 1024 * 1024;
pub(super) struct Candidate {
    pub checkpoint: Checkpoint,
    pub archive: TreeArchive,
}
impl Candidate {
    pub fn load(engine: &dyn ICheckpointEngine, checkpoint: Checkpoint) -> Result<Self> {
        if checkpoint.manifest.entries.len() > MAX_FILES
            || checkpoint.manifest.total_bytes > MAX_BYTES
        {
            return Err(Error::Unsupported(
                "edit sessions support at most 4096 entries and 8 MiB selected bytes".into(),
            ));
        }
        let bytes = engine.export_checkpoint_archive(checkpoint.reference())?;
        if bytes.len() > MAX_ARCHIVE {
            return Err(Error::InvalidInput("edit archive exceeds 32 MiB".into()));
        }
        let archive = serde_json::from_slice(&bytes)?;
        Ok(Self {
            checkpoint,
            archive,
        })
    }
    pub fn prepare(&self, change: EditChange) -> Result<(TreeArchive, Vec<u8>)> {
        change.validate()?;
        let mut archive = self.archive.clone();
        match change {
            EditChange::Create {
                path,
                bytes,
                executable,
            } => {
                if find(&archive, &path).is_some() {
                    return Err(Error::Conflict(
                        "candidate creation destination already exists".into(),
                    ));
                }
                archive.entries.push(regular(path, bytes, executable));
            }
            EditChange::Replace {
                path,
                bytes,
                executable,
            } => {
                let index = find(&archive, &path)
                    .ok_or_else(|| Error::NotFound("candidate replacement path".into()))?;
                if archive.entries[index].file.kind != EntryKind::File {
                    return Err(Error::Unsupported(
                        "candidate replacement cannot follow a symlink".into(),
                    ));
                }
                archive.entries[index] = regular(path, bytes, executable);
            }
            EditChange::Delete { path } => {
                let index = find(&archive, &path)
                    .ok_or_else(|| Error::NotFound("candidate deletion path".into()))?;
                archive.entries.remove(index);
            }
            EditChange::Rename { from, to } => {
                let index = find(&archive, &from)
                    .ok_or_else(|| Error::NotFound("candidate rename source".into()))?;
                if find(&archive, &to).is_some() {
                    return Err(Error::Conflict(
                        "candidate rename destination already exists".into(),
                    ));
                }
                archive.entries[index].file.path = to.as_str().to_owned();
            }
        }
        if archive.entries.len() > MAX_FILES
            || archive
                .entries
                .iter()
                .try_fold(0u64, |total, entry| total.checked_add(entry.file.size))
                .is_none_or(|total| total > MAX_BYTES)
        {
            return Err(Error::InvalidInput(
                "candidate exceeds edit-session bounds".into(),
            ));
        }
        archive
            .entries
            .sort_by(|a, b| a.file.path.cmp(&b.file.path));
        let bytes = serde_json::to_vec(&archive)?;
        if bytes.len() > MAX_ARCHIVE {
            return Err(Error::InvalidInput("edit archive exceeds 32 MiB".into()));
        }
        Ok((archive, bytes))
    }
    pub fn capture(
        engine: &dyn ICheckpointEngine,
        parent: &Checkpoint,
        archive: TreeArchive,
        bytes: Vec<u8>,
    ) -> Result<Self> {
        // Existing engine validates mandatory exclusions, portable paths, modes,
        // hashes, overlap/case collisions and the entire symlink graph.
        let checkpoint = engine.capture_archive(
            &bytes,
            CaptureOptions {
                parent: Some(parent.reference().clone()),
                ..Default::default()
            },
        )?;
        Ok(Self {
            checkpoint,
            archive,
        })
    }
}
fn find(archive: &TreeArchive, path: &WorkspacePath) -> Option<usize> {
    archive
        .entries
        .iter()
        .position(|entry| entry.file.path == path.as_str())
}
fn regular(path: WorkspacePath, bytes: Vec<u8>, executable: bool) -> TreeArchiveEntry {
    TreeArchiveEntry {
        file: FileEntry {
            path: path.as_str().to_owned(),
            kind: EntryKind::File,
            mode: if executable { 100755 } else { 100644 },
            size: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
        },
        data_base64: STANDARD.encode(bytes),
    }
}
