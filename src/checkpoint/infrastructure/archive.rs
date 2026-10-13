use super::file_tree::*;
use super::filesystem::*;
use super::git_checkpoint_engine::GitCheckpointEngine;
use super::path_policy::*;
use crate::checkpoint::domain::*;
use crate::{Error, Result};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use std::collections::BTreeMap;
use std::path::Path;

impl GitCheckpointEngine {
    pub(super) fn archive_bytes(&self, tree: &Tree) -> Result<Vec<u8>> {
        encode_tree(tree, &self.limits)
    }

    pub(super) fn archive_tree(&self, bytes: &[u8]) -> Result<Tree> {
        decode_tree(bytes, &self.limits)
    }
}

pub(super) fn archive_limit(limits: &CheckpointLimits) -> u64 {
    (limits.max_total_bytes.saturating_add(2) / 3)
        .saturating_mul(4)
        .saturating_add(limits.max_manifest_bytes)
}

pub(super) fn encode_tree(tree: &Tree, limits: &CheckpointLimits) -> Result<Vec<u8>> {
    check_entries(&tree.entries(), limits)?;
    validate_tree_symlinks(tree)?;
    let archive = TreeArchive {
        version: 1,
        entries: tree
            .files
            .values()
            .map(|file| TreeArchiveEntry {
                file: file.entry.clone(),
                data_base64: BASE64_STANDARD.encode(&file.bytes),
            })
            .collect(),
    };
    let bytes = serde_json::to_vec(&archive)?;
    if bytes.len() as u64 > archive_limit(limits) {
        return Err(Error::InvalidInput(
            "encoded tree archive exceeds size limit".into(),
        ));
    }
    Ok(bytes)
}

pub(super) fn decode_tree(bytes: &[u8], limits: &CheckpointLimits) -> Result<Tree> {
    if bytes.len() as u64 > archive_limit(limits) {
        return Err(Error::InvalidInput(
            "encoded tree archive exceeds size limit".into(),
        ));
    }
    let archive: TreeArchive = serde_json::from_slice(bytes)?;
    if archive.version != 1 {
        return Err(Error::Unsupported("unknown tree archive format".into()));
    }
    let entries: Vec<_> = archive.entries.iter().map(|e| e.file.clone()).collect();
    check_entries(&entries, limits)?;
    let mut tree = Tree {
        files: BTreeMap::new(),
        excluded: vec![],
    };
    for archived in archive.entries {
        let entry = archived.file;
        if archived.data_base64.len() as u64 > entry.size.saturating_add(2) / 3 * 4 {
            return Err(Error::InvalidInput(
                "encoded file exceeds declared size".into(),
            ));
        }
        let bytes = BASE64_STANDARD
            .decode(&archived.data_base64)
            .map_err(|_| Error::InvalidInput("invalid archive base64".into()))?;
        if bytes.len() as u64 != entry.size || digest(&bytes) != entry.sha256 {
            return Err(Error::InvalidInput(
                "tree archive checksum/size mismatch".into(),
            ));
        }
        if entry.kind == EntryKind::Symlink {
            validate_symlink(&entry.path, &bytes)?;
        }
        tree.files
            .insert(entry.path.clone(), TreeFile { entry, bytes });
    }
    validate_tree_symlinks(&tree)?;
    Ok(tree)
}

impl GitCheckpointEngine {
    pub(super) fn export_tree_archive_operation(&self, root: &Path) -> Result<Vec<u8>> {
        let _lock = self.lock()?;
        let root = self.checked_root(root)?;
        let tree = self.scan(&root)?;
        if self.scan(&root)?.fingerprint()? != tree.fingerprint()? {
            return Err(Error::Conflict(
                "files changed during archive capture".into(),
            ));
        }
        self.archive_bytes(&tree)
    }

    pub(super) fn import_tree_archive_operation(
        &self,
        bytes: &[u8],
        destination: &Path,
    ) -> Result<RestoreTreeReceipt> {
        let _lock = self.lock()?;
        let tree = self.archive_tree(bytes)?;
        let destination = self.empty_destination(destination)?;
        write_tree(&destination, &tree)?;
        Ok(RestoreTreeReceipt {
            destination,
            fingerprint: tree.fingerprint()?,
            files: tree.files.len(),
        })
    }

    pub(super) fn export_checkpoint_archive_operation(
        &self,
        reference: &CheckpointRef,
    ) -> Result<Vec<u8>> {
        let _lock = self.lock()?;
        self.archive_bytes(&self.load_tree(reference)?)
    }

    pub(super) fn capture_archive_operation(
        &self,
        bytes: &[u8],
        options: CaptureOptions,
    ) -> Result<Checkpoint> {
        let _lock = self.lock()?;
        self.save_tree(&self.archive_tree(bytes)?, options)
    }
}
