//! Native target-side transport for the shared, validated tree archive format.
use super::archive::encode_tree;
use super::file_tree::digest;
use super::git_checkpoint_engine::GitCheckpointEngine;
use super::path_policy::mandatory_exclusion;
use super::remote_import::{import_locked, read_receipt, ImportStatus};
use super::remote_storage::{atomic_bytes, checked_path, checked_project, lock_project};
use super::scanner::scan;
use crate::checkpoint::domain::CheckpointLimits;
use crate::{Error, Result, TreeExportReceipt, TreeImportReceipt, TreeTransfer};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use tempfile::tempdir;

/// Export selected project files to the archive identified by `transfer`.
///
/// The caller must stop project writers before calling. A second scan detects
/// capture-time changes but does not replace that boundary. Git is used only to
/// inspect tracked paths; the source repository and its index are never changed.
/// The returned byte count and digest let the controller verify downloaded data.
pub fn export_remote_tree(project: &Path, transfer: &TreeTransfer) -> Result<TreeExportReceipt> {
    let root = checked_project(project)?;
    let _lock = lock_project(&root)?;
    let limits = CheckpointLimits::default();
    let archive = checked_path(&root, transfer.relative_archive_path().as_str(), true)?;
    let temporary = tempdir()?;
    let engine = GitCheckpointEngine::new("git", temporary.path().join("checkpoint"))?;
    let mut tracked = engine.tracked(&root)?;
    if let Some(receipt) = read_receipt(&root, &limits)? {
        if receipt.status != ImportStatus::Complete {
            return Err(Error::Conflict(
                "initial tree import is incomplete; retry import before export".into(),
            ));
        }
        tracked.extend(receipt.selected_paths);
    }
    preflight_filesystem(&root, &limits)?;
    let tree = scan(&root, &tracked, &limits)?;
    if scan(&root, &tracked, &limits)?.fingerprint()? != tree.fingerprint()? {
        return Err(Error::Conflict(
            "files changed during export; stop writers and retry".into(),
        ));
    }
    let bytes = encode_tree(&tree, &limits)?;
    atomic_bytes(&archive, &bytes)?;
    Ok(TreeExportReceipt {
        bytes: bytes.len() as u64,
        sha256: digest(&bytes),
        files: tree.files.len(),
    })
}

/// Import a validated archive into an empty project or resume its matching retry.
///
/// A durable receipt is written before publishing files. Retrying verifies all
/// existing selected files before publishing anything further, and fails if a
/// user edited the imported tree. New files are published atomically with
/// new-only operations. The caller must keep project writers stopped throughout.
pub fn import_remote_tree(project: &Path, transfer: &TreeTransfer) -> Result<TreeImportReceipt> {
    let root = checked_project(project)?;
    let _lock = lock_project(&root)?;
    let archive = checked_path(&root, transfer.relative_archive_path().as_str(), false)?;
    import_locked(&root, &archive)
}

/// Remove only this transfer's staging directory; missing staging is success.
///
/// Symlink ancestors and a symlink in place of the transfer directory are
/// rejected. Imported project files and the durable import receipt are retained.
pub fn cleanup_transfer(project: &Path, transfer: &TreeTransfer) -> Result<()> {
    let root = checked_project(project)?;
    let _lock = lock_project(&root)?;
    let directory = match checked_path(&root, transfer.relative_directory().as_str(), false) {
        Ok(path) => path,
        Err(Error::Io(error)) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(&directory)?;
            fs::File::open(directory.parent().unwrap())?.sync_all()?;
            Ok(())
        }
        Ok(_) => Err(Error::Conflict(
            "transfer directory must be a real directory".into(),
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Reject unsupported entries even when an ignore rule would hide them. Git's
/// file listing omits sockets/FIFOs; silently omitting them would lose information.
pub(super) fn preflight_filesystem(root: &Path, limits: &CheckpointLimits) -> Result<()> {
    let mut pending = vec![root.to_owned()];
    let mut visited = 0usize;
    while let Some(directory) = pending.pop() {
        for child in fs::read_dir(directory)? {
            let child = child?;
            let path = child.path();
            let name = path.strip_prefix(root).unwrap().to_str().ok_or_else(|| {
                Error::Unsupported("non-UTF-8 file paths are not portable".into())
            })?;
            if mandatory_exclusion(name) {
                continue;
            }
            visited += 1;
            if visited > limits.max_files.saturating_mul(20).max(1000) {
                return Err(Error::InvalidInput(
                    "filesystem traversal exceeds limit".into(),
                ));
            }
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.is_dir() {
                pending.push(path);
            } else if !metadata.is_file() && !metadata.file_type().is_symlink() {
                return Err(Error::Unsupported(format!(
                    "unsupported filesystem feature: {name}"
                )));
            }
        }
    }
    Ok(())
}
