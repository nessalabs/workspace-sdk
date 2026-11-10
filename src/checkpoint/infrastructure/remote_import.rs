use super::archive::{archive_limit, decode_tree};
use super::file_tree::{digest, Tree, TreeFile};
use super::filesystem::{read_regular, safe_target, sync_publication_ancestors, PublicationStep};
use super::path_policy::{mandatory_exclusion, relative_path};
use super::remote_storage::{atomic_bytes_with, checked_path, IMPORT_RECEIPT};
use super::remote_tree::preflight_filesystem;
use super::scanner::scan;
use crate::checkpoint::domain::{CheckpointLimits, EntryKind};
use crate::{Error, Result, TreeImportReceipt};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::io::{ErrorKind, Write};
#[cfg(unix)]
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::Path;
#[cfg(unix)]
use std::str::from_utf8;
use tempfile::NamedTempFile;

#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ImportStatus {
    Pending,
    Complete,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ImportJournal {
    version: u32,
    sha256: String,
    files: usize,
    pub selected_paths: BTreeSet<String>,
    pub status: ImportStatus,
}

pub(super) fn read_receipt(
    root: &Path,
    limits: &CheckpointLimits,
) -> Result<Option<ImportJournal>> {
    let path = checked_path(root, IMPORT_RECEIPT, false)?;
    let bytes = match read_regular(&path, limits.max_manifest_bytes) {
        Ok(bytes) => bytes,
        Err(Error::Io(error)) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let receipt: ImportJournal = serde_json::from_slice(&bytes)?;
    if receipt.version != 1
        || receipt.files != receipt.selected_paths.len()
        || receipt.files > limits.max_files
        || receipt.sha256.len() != 64
        || !receipt
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(Error::State("invalid tree import receipt".into()));
    }
    for path in &receipt.selected_paths {
        relative_path(path)?;
        if mandatory_exclusion(path) {
            return Err(Error::State(
                "tree import receipt contains an excluded path".into(),
            ));
        }
    }
    Ok(Some(receipt))
}

pub(super) fn import_locked(root: &Path, archive: &Path) -> Result<TreeImportReceipt> {
    import_locked_with(root, archive, |step| step.perform())
}

fn import_locked_with(
    root: &Path,
    archive: &Path,
    mut perform: impl FnMut(&PublicationStep) -> Result<()>,
) -> Result<TreeImportReceipt> {
    let limits = CheckpointLimits::default();
    let bytes = read_regular(archive, archive_limit(&limits))?;
    let tree = decode_tree(&bytes, &limits)?;
    let selected: BTreeSet<_> = tree.files.keys().cloned().collect();
    let checksum = digest(&bytes);
    let previous = read_receipt(root, &limits)?;
    if let Some(receipt) = previous.as_ref() {
        if receipt.sha256 != checksum || receipt.selected_paths != selected {
            return Err(Error::Conflict(
                "restore receipt belongs to a different checkpoint".into(),
            ));
        }
        preflight_filesystem(root, &limits)?;
        let actual = scan(root, &selected, &limits)?;
        for (path, file) in &actual.files {
            if tree
                .files
                .get(path)
                .is_none_or(|expected| expected.entry != file.entry)
            {
                return Err(Error::Conflict(
                    "restored tree differs from checkpoint".into(),
                ));
            }
        }
        if receipt.status == ImportStatus::Complete && actual.entries() != tree.entries() {
            return Err(Error::Conflict("restored tree changed after import".into()));
        }
    } else if fs::read_dir(root)?
        .any(|entry| entry.map_or(true, |entry| entry.file_name() != ".workspace-sdk"))
    {
        return Err(Error::Conflict(
            "restore destination contains existing project files".into(),
        ));
    }
    // Check every selected path directly, including files now hidden by changed
    // ignore rules, before allowing any additional file publication.
    let mut missing = Vec::new();
    for file in tree.files.values() {
        if !matches_existing(root, file, &limits)? {
            missing.push(file);
        }
    }
    let receipt_path = checked_path(root, IMPORT_RECEIPT, false)?;
    if previous
        .as_ref()
        .is_some_and(|receipt| receipt.status == ImportStatus::Complete)
    {
        if !missing.is_empty() {
            return Err(Error::Conflict("restored tree changed after import".into()));
        }
        // A visible Complete receipt can follow a failed directory fsync.
        // Re-verify above, then repeat the durability boundary before ACK.
        sync_imported_tree(root, &tree, Some(&receipt_path), &mut perform)?;
        return Ok(TreeImportReceipt {
            files: tree.files.len(),
        });
    }
    let mut receipt = ImportJournal {
        version: 1,
        sha256: checksum,
        files: tree.files.len(),
        selected_paths: selected,
        status: ImportStatus::Pending,
    };
    save_receipt(&receipt_path, &receipt, &limits, &mut perform)?;
    for file in missing {
        publish_file(root, archive.parent().unwrap(), file, &mut perform)?;
    }
    validate_complete(root, &tree, &limits)?;
    // Matching files on a Pending retry may have been published immediately
    // before an earlier fsync failure. Matching bytes alone are not durability.
    sync_imported_tree(root, &tree, None, &mut perform)?;
    receipt.status = ImportStatus::Complete;
    save_receipt(&receipt_path, &receipt, &limits, &mut perform)?;
    Ok(TreeImportReceipt {
        files: tree.files.len(),
    })
}

fn save_receipt(
    path: &Path,
    receipt: &ImportJournal,
    limits: &CheckpointLimits,
    perform: &mut impl FnMut(&PublicationStep) -> Result<()>,
) -> Result<()> {
    let bytes = serde_json::to_vec(receipt)?;
    if bytes.len() as u64 > limits.max_manifest_bytes {
        return Err(Error::InvalidInput(
            "tree import receipt exceeds metadata limit".into(),
        ));
    }
    atomic_bytes_with(path, &bytes, &mut *perform)?;
    // A prior creation failure can leave a visible metadata directory whose
    // own name is not durable. Finish intent ancestry before publishing files.
    sync_publication_ancestors(path.parent().unwrap(), perform)
}

fn matches_existing(root: &Path, file: &TreeFile, limits: &CheckpointLimits) -> Result<bool> {
    let path = match checked_path(root, &file.entry.path, false) {
        Ok(path) => path,
        Err(Error::Io(error)) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let matches = if file.entry.kind == EntryKind::Symlink {
        metadata.file_type().is_symlink()
            && fs::read_link(&path)?.to_str().map(str::as_bytes) == Some(file.bytes.as_slice())
    } else if metadata.is_file() && !metadata.file_type().is_symlink() {
        #[cfg(unix)]
        let executable = { metadata.permissions().mode() & 0o111 != 0 };
        #[cfg(not(unix))]
        let executable = false;
        executable == (file.entry.mode == 100755)
            && read_regular(&path, limits.max_file_bytes)? == file.bytes
    } else {
        false
    };
    if !matches {
        return Err(Error::Conflict(format!(
            "partial restored path differs from checkpoint: {}",
            file.entry.path
        )));
    }
    Ok(true)
}

fn publish_file(
    root: &Path,
    staging: &Path,
    file: &TreeFile,
    perform: &mut impl FnMut(&PublicationStep) -> Result<()>,
) -> Result<()> {
    let target = safe_target(root, &file.entry.path)?;
    if file.entry.kind == EntryKind::Symlink {
        #[cfg(unix)]
        symlink(from_utf8(&file.bytes).unwrap(), &target)?;
        #[cfg(not(unix))]
        return Err(Error::Unsupported(
            "symlink restoration requires Unix".into(),
        ));
    } else {
        let mut temporary = NamedTempFile::new_in(staging)?;
        temporary.write_all(&file.bytes)?;
        #[cfg(unix)]
        {
            temporary
                .as_file()
                .set_permissions(fs::Permissions::from_mode(if file.entry.mode == 100755 {
                    0o755
                } else {
                    0o644
                }))?;
        }
        temporary.as_file().sync_all()?;
        // Hard-link publication refuses to replace a path created after preflight.
        fs::hard_link(temporary.path(), &target)?;
        perform(&PublicationStep::SyncFile(target.clone()))?;
    }
    let mut parent = target.parent();
    while let Some(directory) = parent {
        perform(&PublicationStep::SyncDirectory(directory.to_owned()))?;
        if directory == root {
            break;
        }
        parent = directory.parent();
    }
    Ok(())
}

/// Flush only verified selected regular files; symlinks require their parents,
/// never an open of the link or its target. Repeat this on both retry states.
fn sync_imported_tree(
    root: &Path,
    tree: &Tree,
    receipt: Option<&Path>,
    perform: &mut impl FnMut(&PublicationStep) -> Result<()>,
) -> Result<()> {
    for file in tree.files.values() {
        let path = checked_path(root, &file.entry.path, false)?;
        if file.entry.kind == EntryKind::File {
            perform(&PublicationStep::SyncFile(path.clone()))?;
        }
        for parent in path
            .ancestors()
            .skip(1)
            .take_while(|parent| *parent != root)
        {
            perform(&PublicationStep::SyncDirectory(parent.to_owned()))?;
        }
    }
    if let Some(path) = receipt {
        perform(&PublicationStep::SyncFile(path.to_owned()))?;
        for parent in path
            .ancestors()
            .skip(1)
            .take_while(|parent| *parent != root)
        {
            perform(&PublicationStep::SyncDirectory(parent.to_owned()))?;
        }
    }
    // Walk parents directly rather than copying all path prefixes into a set.
    // Shared parents may repeat; only one copied prefix is retained at a time.
    // Include the project root even for an empty tree, then its canonical
    // ancestry: an earlier interrupted import may have left entries unsynced.
    sync_publication_ancestors(root, perform)
}

fn validate_complete(root: &Path, expected: &Tree, limits: &CheckpointLimits) -> Result<()> {
    for file in expected.files.values() {
        if !matches_existing(root, file, limits)? {
            return Err(Error::Conflict(
                "imported tree changed before completion".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "remote_import_tests.rs"]
mod tests;
