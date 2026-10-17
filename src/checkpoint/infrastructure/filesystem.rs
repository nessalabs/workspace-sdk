use super::file_tree::*;
use super::path_policy::*;
use crate::checkpoint::domain::*;
use crate::{Error, Result};
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{symlink, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::str::from_utf8;

/// Concrete syscall boundary for local publication ordering tests. The production
/// callback executes each step immediately; no mutable process-global failpoint.
#[derive(Clone, Debug)]
pub(super) enum PublicationStep {
    SyncFile(PathBuf),
    SyncDirectory(PathBuf),
    CreateDirectory(PathBuf),
    Rename { from: PathBuf, to: PathBuf },
}

impl PublicationStep {
    pub(super) fn perform(&self) -> Result<()> {
        match self {
            Self::SyncFile(path) | Self::SyncDirectory(path) => {
                let directory = matches!(self, Self::SyncDirectory(_));
                let mut options = OpenOptions::new();
                options.read(true);
                #[cfg(unix)]
                {
                    let kind = if directory {
                        libc::O_DIRECTORY
                    } else {
                        libc::O_NONBLOCK
                    };
                    options.custom_flags(libc::O_NOFOLLOW | kind);
                }
                let file = options.open(path)?;
                let metadata = file.metadata()?;
                if (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
                    return Err(Error::InvalidInput(
                        "checkpoint publication requires real directories and regular files".into(),
                    ));
                }
                file.sync_all()?;
            }
            Self::CreateDirectory(path) => match fs::create_dir(path) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    real_publication_directory(path)?;
                }
                Err(error) => return Err(error.into()),
            },
            Self::Rename { from, to } => {
                fs::rename(from, to)?;
            }
        }
        Ok(())
    }
}

pub(super) fn publish_checkpoint_directory_with(
    staged: &Path,
    destination: &Path,
    limits: &CheckpointLimits,
    mut perform: impl FnMut(&PublicationStep) -> Result<()>,
) -> Result<()> {
    let parent = destination
        .parent()
        .ok_or_else(|| Error::State("checkpoint parent missing".into()))?;
    if staged.parent() != Some(parent) || destination.exists() {
        return Err(Error::Conflict(
            "checkpoint publication requires an unused sibling destination".into(),
        ));
    }
    real_publication_directory(parent)?;
    sync_artifact_tree(staged, limits, &mut perform)?;
    perform(&PublicationStep::Rename {
        from: staged.to_owned(),
        to: destination.to_owned(),
    })?;
    // A visible renamed artifact is still an uncertain publication until this
    // directory entry is synced. Never erase it on a post-rename failure.
    sync_publication_ancestors(parent, &mut perform)
}

pub(super) fn sync_checkpoint_directory_with(
    directory: &Path,
    limits: &CheckpointLimits,
    mut perform: impl FnMut(&PublicationStep) -> Result<()>,
) -> Result<()> {
    sync_artifact_tree(directory, limits, &mut perform)?;
    let parent = directory
        .parent()
        .ok_or_else(|| Error::State("checkpoint parent missing".into()))?;
    real_publication_directory(parent)?;
    sync_publication_ancestors(parent, &mut perform)
}

fn real_publication_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::InvalidInput(
            "checkpoint publication directory must be real".into(),
        ));
    }
    Ok(())
}

/// Flush the whole canonical ancestry even on retries of already visible paths.
/// A prior interrupted creation may have left any of those entries unsynced.
pub(super) fn create_checkpoint_directory_with(
    path: &Path,
    mut perform: impl FnMut(&PublicationStep) -> Result<()>,
) -> Result<PathBuf> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(Error::InvalidInput(
            "checkpoint state cannot be a symlink".into(),
        ));
    }
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut anchor = absolute.as_path();
    let mut missing = vec![];
    loop {
        match fs::metadata(anchor) {
            Ok(metadata) if metadata.is_dir() => break,
            Ok(_) => {
                return Err(Error::InvalidInput(
                    "checkpoint state ancestor must be a directory".into(),
                ))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                missing.push(
                    anchor
                        .file_name()
                        .ok_or_else(|| {
                            Error::InvalidInput("invalid checkpoint directory path".into())
                        })?
                        .to_os_string(),
                );
                if missing.len() > 4096 {
                    return Err(Error::InvalidInput(
                        "checkpoint state path exceeds depth limit".into(),
                    ));
                }
                anchor = anchor.parent().ok_or_else(|| {
                    Error::InvalidInput("checkpoint state ancestor missing".into())
                })?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    // Preserve supported ancestor symlinks, then use canonical paths for every
    // created directory and ancestor sync. The state leaf itself cannot be a link.
    let mut directory = fs::canonicalize(anchor)?;
    for component in missing.into_iter().rev() {
        directory.push(component);
        perform(&PublicationStep::CreateDirectory(directory.clone()))?;
    }
    directory = fs::canonicalize(directory)?;
    sync_publication_ancestors(&directory, &mut perform)?;
    Ok(directory)
}

fn sync_publication_ancestors(
    directory: &Path,
    perform: &mut impl FnMut(&PublicationStep) -> Result<()>,
) -> Result<()> {
    let directory = fs::canonicalize(directory)?;
    for (depth, ancestor) in directory.ancestors().enumerate() {
        if depth > 4096 {
            return Err(Error::InvalidInput(
                "checkpoint state path exceeds depth limit".into(),
            ));
        }
        real_publication_directory(ancestor)?;
        perform(&PublicationStep::SyncDirectory(ancestor.to_owned()))?;
    }
    Ok(())
}

struct ArtifactBudget {
    entries: usize,
    bytes: u64,
}

fn sync_artifact_tree(
    root: &Path,
    limits: &CheckpointLimits,
    perform: &mut impl FnMut(&PublicationStep) -> Result<()>,
) -> Result<()> {
    // Entry allowance matches import's per-file object expansion plus fixed Git
    // metadata. Byte allowance covers bundle, loose objects, metadata and slack;
    // this walks SDK-owned artifacts, never the source project filesystem.
    let mut budget = ArtifactBudget {
        entries: limits.max_files.saturating_mul(4096).saturating_add(1024),
        bytes: limits
            .max_bundle_bytes
            .saturating_add(limits.max_total_bytes)
            .saturating_add(limits.max_manifest_bytes)
            .saturating_mul(2)
            .saturating_add((limits.max_files as u64).saturating_mul(4096)),
    };
    sync_artifact_directory(root, 0, &mut budget, perform)
}

fn sync_artifact_directory(
    directory: &Path,
    depth: usize,
    budget: &mut ArtifactBudget,
    perform: &mut impl FnMut(&PublicationStep) -> Result<()>,
) -> Result<()> {
    if depth > 64 {
        return Err(Error::InvalidInput(
            "checkpoint artifact exceeds directory depth limit".into(),
        ));
    }
    real_publication_directory(directory)?;
    for child in fs::read_dir(directory)? {
        budget.entries = budget
            .entries
            .checked_sub(1)
            .ok_or_else(|| Error::InvalidInput("checkpoint artifact exceeds entry limit".into()))?;
        let path = child?.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(Error::InvalidInput(
                "checkpoint artifact contains a symlink".into(),
            ));
        }
        if metadata.is_dir() {
            sync_artifact_directory(&path, depth + 1, budget, perform)?;
        } else if metadata.is_file() {
            budget.bytes = budget.bytes.checked_sub(metadata.len()).ok_or_else(|| {
                Error::InvalidInput("checkpoint artifact exceeds byte limit".into())
            })?;
            perform(&PublicationStep::SyncFile(path))?;
        } else {
            return Err(Error::InvalidInput(
                "checkpoint artifact contains a special file".into(),
            ));
        }
    }
    perform(&PublicationStep::SyncDirectory(directory.to_owned()))
}

pub(crate) fn read_regular(path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        // O_NONBLOCK prevents a replaced FIFO from blocking before metadata can
        // reject it. It does not change ordinary regular-file reads.
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > max_bytes {
        return Err(Error::InvalidInput(
            "file exceeds size limit or is not regular".into(),
        ));
    }
    let mut bytes = vec![];
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(Error::InvalidInput("file exceeds size limit".into()));
    }
    Ok(bytes)
}

pub(crate) fn safe_target(root: &Path, relative: &str) -> Result<PathBuf> {
    relative_path(relative)?;
    let root_metadata = fs::symlink_metadata(root)?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err(Error::InvalidInput(
            "destination root must be a real directory".into(),
        ));
    }
    let mut current = root.to_owned();
    let path = Path::new(relative);
    for component in path.parent().unwrap_or(Path::new("")).components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(m) if !m.is_dir() || m.file_type().is_symlink() => {
                return Err(Error::Conflict(format!(
                    "destination ancestor is not a directory: {}",
                    current.display()
                )))
            }
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => fs::create_dir(&current)?,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(root.join(path))
}

pub(crate) fn write_tree(root: &Path, tree: &Tree) -> Result<()> {
    validate_tree_symlinks(tree)?;
    for file in tree.files.values() {
        let target = safe_target(root, &file.entry.path)?;
        // New-only opens prevent replacing a symlink or a concurrently created file.
        if file.entry.kind == EntryKind::Symlink {
            validate_symlink(&file.entry.path, &file.bytes)?;
            #[cfg(unix)]
            symlink(from_utf8(&file.bytes).unwrap(), &target)?;
            #[cfg(not(unix))]
            return Err(Error::Unsupported(
                "symlink restoration requires Unix".into(),
            ));
        } else {
            let mut out = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)?;
            out.write_all(&file.bytes)?;
            #[cfg(unix)]
            {
                out.set_permissions(fs::Permissions::from_mode(if file.entry.mode == 100755 {
                    0o755
                } else {
                    0o644
                }))?;
            }
            out.sync_all()?;
        }
    }
    Ok(())
}

pub(crate) fn remove_selected(root: &Path, entries: &[FileEntry], recovering: bool) -> Result<()> {
    let mut directories = BTreeSet::new();
    let selected: BTreeSet<_> = entries.iter().map(|e| e.path.as_str()).collect();
    for entry in entries {
        relative_path(&entry.path)?;
        let mut skip = false;
        let mut ancestor = PathBuf::new();
        for component in Path::new(&entry.path)
            .parent()
            .unwrap_or(Path::new(""))
            .components()
        {
            ancestor.push(component);
            match fs::symlink_metadata(root.join(&ancestor)) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
                Ok(_) if recovering && selected.contains(ancestor.to_str().unwrap_or("")) => {
                    skip = true;
                    break;
                }
                Ok(_) => {
                    return Err(Error::Conflict(
                        "destination ancestor changed during apply".into(),
                    ))
                }
                Err(e) if e.kind() == ErrorKind::NotFound => {
                    skip = true;
                    break;
                }
                Err(e) => return Err(e.into()),
            }
        }
        if skip {
            continue;
        }
        let path = root.join(&entry.path);
        match fs::symlink_metadata(&path) {
            Ok(m) if m.is_dir() => match fs::remove_dir(&path) {
                Ok(_) => {}
                Err(e) if recovering && e.kind() == ErrorKind::DirectoryNotEmpty => {}
                Err(e) => return Err(e.into()),
            },
            Ok(_) => fs::remove_file(&path)?,
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let mut parent = Path::new(&entry.path).parent();
        while let Some(p) = parent {
            if !p.as_os_str().is_empty() {
                directories.insert(p.to_owned());
            }
            parent = p.parent();
        }
    }
    for directory in directories.iter().rev() {
        match fs::remove_dir(root.join(directory)) {
            Ok(_) => {}
            Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::DirectoryNotEmpty) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod publication_tests {
    use super::*;

    fn staged(root: &Path) -> PathBuf {
        let stage = root.join("stage");
        fs::create_dir_all(stage.join("repo/objects/aa")).unwrap();
        fs::write(stage.join("repo/objects/aa/object"), b"object").unwrap();
        fs::write(stage.join("snapshot.bundle"), b"bundle").unwrap();
        fs::write(stage.join("manifest.json"), b"manifest").unwrap();
        stage
    }
    fn limits() -> CheckpointLimits {
        CheckpointLimits::default()
    }

    #[test]
    fn publication_syncs_files_and_directories_bottom_up_before_rename_and_parent_sync() {
        let root = tempfile::tempdir().unwrap();
        let root_path = fs::canonicalize(root.path()).unwrap();
        let stage = staged(root_path.as_path());
        let destination = root_path.as_path().join("published");
        let mut steps = vec![];
        publish_checkpoint_directory_with(&stage, &destination, &limits(), |step| {
            steps.push(step.clone());
            step.perform()
        })
        .unwrap();
        let rename = steps
            .iter()
            .position(|step| matches!(step, PublicationStep::Rename { .. }))
            .unwrap();
        assert!(steps[..rename].iter().all(|step| matches!(
            step,
            PublicationStep::SyncFile(_) | PublicationStep::SyncDirectory(_)
        )));
        assert!(
            matches!(&steps[rename - 1], PublicationStep::SyncDirectory(path) if path == &stage)
        );
        assert!(
            matches!(&steps[rename + 1], PublicationStep::SyncDirectory(path) if path == root_path.as_path())
        );
        assert!(
            matches!(steps.last(), Some(PublicationStep::SyncDirectory(path)) if path.parent().is_none())
        );
        for (index, step) in steps[..rename].iter().enumerate() {
            let path = match step {
                PublicationStep::SyncFile(path) | PublicationStep::SyncDirectory(path) => path,
                _ => unreachable!(),
            };
            if path != &stage {
                let parent = path.parent().unwrap();
                let parent_index = steps.iter().position(|step| matches!(step, PublicationStep::SyncDirectory(path) if path == parent)).unwrap();
                assert!(
                    index < parent_index,
                    "child must be synced before its directory"
                );
            }
        }
        assert_eq!(
            fs::read(destination.join("snapshot.bundle")).unwrap(),
            b"bundle"
        );
        assert!(!stage.exists());
    }

    #[test]
    fn every_publication_failure_refuses_acknowledgement_and_keeps_previous_artifact() {
        for failure in [
            "file",
            "nested-directory",
            "root-directory",
            "rename",
            "final-parent",
        ] {
            let root = tempfile::tempdir().unwrap();
            let root_path = fs::canonicalize(root.path()).unwrap();
            let stage = staged(root_path.as_path());
            let destination = root_path.as_path().join("published");
            fs::write(root_path.as_path().join("previous"), b"previous").unwrap();
            let mut injected = false;
            let result =
                publish_checkpoint_directory_with(&stage, &destination, &limits(), |step| {
                    let selected = match step {
                        PublicationStep::SyncFile(_) => failure == "file",
                        PublicationStep::SyncDirectory(path) if path == root_path.as_path() => {
                            failure == "final-parent"
                        }
                        PublicationStep::SyncDirectory(path) if path == &stage => {
                            failure == "root-directory"
                        }
                        PublicationStep::SyncDirectory(_) => failure == "nested-directory",
                        PublicationStep::Rename { .. } => failure == "rename",
                        _ => false,
                    };
                    if selected {
                        injected = true;
                        return Err(Error::State("injected publication boundary".into()));
                    }
                    step.perform()
                });
            assert!(
                injected && result.is_err(),
                "missing failure boundary: {failure}"
            );
            assert_eq!(destination.exists(), failure == "final-parent");
            assert_eq!(
                fs::read(root_path.as_path().join("previous")).unwrap(),
                b"previous"
            );
            if failure == "final-parent" {
                let mut retried_parent = false;
                sync_checkpoint_directory_with(&destination, &limits(), |step| {
                    if matches!(step, PublicationStep::SyncDirectory(path) if path == root_path.as_path()) {
                        retried_parent = true;
                    }
                    step.perform()
                })
                .unwrap();
                assert!(retried_parent);
            }
        }
    }

    fn assert_artifact_bound_refuses_publication(
        stage: &Path,
        limits: &CheckpointLimits,
        expected_error: &str,
    ) {
        let parent = stage.parent().unwrap();
        let previous = parent.join("previous");
        fs::create_dir(&previous).unwrap();
        fs::write(previous.join("snapshot.bundle"), b"previous bundle").unwrap();
        let destination = parent.join("published");
        let mut rename_attempted = false;
        let result = publish_checkpoint_directory_with(stage, &destination, limits, |step| {
            // Observe the publication boundary without issuing unrelated fsyncs.
            // The walker still inspects real fixture entries and their metadata.
            if matches!(step, PublicationStep::Rename { .. }) {
                rename_attempted = true;
            }
            Ok(())
        });
        assert!(
            matches!(result, Err(Error::InvalidInput(ref message)) if message == expected_error),
            "unexpected publication result: {result:?}"
        );
        assert!(
            !rename_attempted,
            "an invalid artifact must not be published"
        );
        assert!(!destination.exists());
        assert!(stage.is_dir(), "a rejected stage must remain available");
        assert_eq!(
            fs::read(previous.join("snapshot.bundle")).unwrap(),
            b"previous bundle"
        );
    }

    #[test]
    fn artifact_entry_budget_exhaustion_refuses_publication_and_preserves_previous() {
        let root = tempfile::tempdir().unwrap();
        let stage = root.path().join("stage");
        fs::create_dir(&stage).unwrap();
        // One selected file permits 5120 expanded artifact metadata entries.
        // Empty files isolate the entry budget from the content-byte budget.
        for index in 0..5121 {
            fs::write(stage.join(format!("metadata-{index}")), b"").unwrap();
        }
        let limits = CheckpointLimits {
            max_files: 1,
            ..limits()
        };
        assert_artifact_bound_refuses_publication(
            &stage,
            &limits,
            "checkpoint artifact exceeds entry limit",
        );
    }

    #[test]
    fn artifact_aggregate_byte_budget_exhaustion_refuses_publication_and_preserves_previous() {
        let root = tempfile::tempdir().unwrap();
        let stage = root.path().join("stage");
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("object-a"), vec![b'a'; 2052]).unwrap();
        fs::write(stage.join("object-b"), vec![b'b'; 2052]).unwrap();
        // The artifact byte allowance is 4102 bytes. Either file fits alone,
        // while their 4104-byte aggregate exceeds it regardless of read order.
        let limits = CheckpointLimits {
            max_files: 1,
            max_bundle_bytes: 1,
            max_total_bytes: 1,
            max_manifest_bytes: 1,
            ..limits()
        };
        assert_artifact_bound_refuses_publication(
            &stage,
            &limits,
            "checkpoint artifact exceeds byte limit",
        );
    }

    #[test]
    fn excessive_owned_artifact_depth_refuses_publication_and_preserves_previous() {
        let root = tempfile::tempdir().unwrap();
        let stage = root.path().join("stage");
        let mut nested = stage.clone();
        // The artifact root is depth zero; its 65th descendant is unsupported.
        for _ in 0..65 {
            nested.push("d");
        }
        fs::create_dir_all(nested).unwrap();
        assert_artifact_bound_refuses_publication(
            &stage,
            &limits(),
            "checkpoint artifact exceeds directory depth limit",
        );
    }

    #[test]
    fn directory_creation_syncs_each_new_directory_and_preexisting_anchor() {
        let root = tempfile::tempdir().unwrap();
        let root_path = fs::canonicalize(root.path()).unwrap();
        let destination = root_path.as_path().join("state/nested/checkpoints");
        let mut synced = vec![];
        let actual = create_checkpoint_directory_with(&destination, |step| {
            if let PublicationStep::SyncDirectory(path) = step {
                synced.push(path.clone());
            }
            step.perform()
        })
        .unwrap();
        assert_eq!(actual, destination);
        assert_eq!(
            synced,
            actual
                .ancestors()
                .map(Path::to_path_buf)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn ancestor_sync_failure_is_not_a_successful_directory_creation() {
        let root = tempfile::tempdir().unwrap();
        let root_path = fs::canonicalize(root.path()).unwrap();
        let destination = root_path.as_path().join("state/checkpoints");
        assert!(create_checkpoint_directory_with(&destination, |step| {
            if matches!(step, PublicationStep::SyncDirectory(path) if path == root_path.as_path()) {
                return Err(Error::State("injected ancestor sync failure".into()));
            }
            step.perform()
        })
        .is_err());
    }

    #[test]
    fn retry_after_interrupted_directory_creation_resyncs_existing_ancestors() {
        let root = tempfile::tempdir().unwrap();
        let root_path = fs::canonicalize(root.path()).unwrap();
        let destination = root_path.join("state/nested/checkpoints");
        assert!(create_checkpoint_directory_with(&destination, |step| {
            if matches!(step, PublicationStep::SyncDirectory(path) if path == &root_path) {
                return Err(Error::State("injected incomplete ancestor sync".into()));
            }
            step.perform()
        })
        .is_err());
        assert!(destination.is_dir());
        let mut synced = vec![];
        create_checkpoint_directory_with(&destination, |step| {
            if let PublicationStep::SyncDirectory(path) = step {
                synced.push(path.clone());
            }
            step.perform()
        })
        .unwrap();
        assert_eq!(
            synced,
            destination
                .ancestors()
                .map(Path::to_path_buf)
                .collect::<Vec<_>>()
        );
    }

    #[cfg(unix)]
    #[test]
    fn publication_rejects_symlinks_and_special_files_without_following_them() {
        let root = tempfile::tempdir().unwrap();
        let root_path = fs::canonicalize(root.path()).unwrap();
        let stage = staged(root_path.as_path());
        let outside = root_path.as_path().join("outside");
        fs::write(&outside, b"outside").unwrap();
        symlink(&outside, stage.join("link")).unwrap();
        assert!(publish_checkpoint_directory_with(
            &stage,
            &root_path.as_path().join("published"),
            &limits(),
            |step| step.perform()
        )
        .is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"outside");
        fs::remove_file(stage.join("link")).unwrap();
        let socket = std::os::unix::net::UnixListener::bind(stage.join("socket")).unwrap();
        assert!(publish_checkpoint_directory_with(
            &stage,
            &root_path.as_path().join("published"),
            &limits(),
            |step| step.perform()
        )
        .is_err());
        drop(socket);
    }
}
