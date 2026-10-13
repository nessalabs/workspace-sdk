use super::path_policy::relative_path;
use crate::{Error, Result};
use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

pub(super) const IMPORT_RECEIPT: &str = ".workspace-sdk/checkpoint-import.json";

pub(super) fn checked_project(project: &Path) -> Result<PathBuf> {
    if !project.is_absolute() {
        return Err(Error::InvalidInput("project root must be absolute".into()));
    }
    real_directory(project)?;
    Ok(fs::canonicalize(project)?)
}

fn real_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::Conflict(format!(
            "directory must be real: {}",
            path.display()
        )));
    }
    Ok(())
}

pub(super) fn checked_path(root: &Path, relative: &str, create: bool) -> Result<PathBuf> {
    relative_path(relative)?;
    real_directory(root)?;
    let mut path = root.to_owned();
    for component in Path::new(relative)
        .parent()
        .unwrap_or(Path::new(""))
        .components()
    {
        path.push(component);
        match real_directory(&path) {
            Ok(()) => {}
            Err(Error::Io(error)) if create && error.kind() == ErrorKind::NotFound => {
                fs::create_dir(&path)?;
                #[cfg(unix)]
                {
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
                }
                File::open(path.parent().unwrap())?.sync_all()?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(root.join(relative))
}

pub(super) fn lock_project(root: &Path) -> Result<File> {
    let path = checked_path(root, ".workspace-sdk/checkpoint-import.lock", true)?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        options.custom_flags(libc::O_NOFOLLOW).mode(0o600);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(Error::InvalidInput(
            "tree import lock is not a regular file".into(),
        ));
    }
    file.lock_exclusive()?;
    Ok(file)
}

pub(super) fn atomic_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            return Err(Error::Conflict(
                "tree metadata output must be a regular file".into(),
            ));
        }
        Err(error) if error.kind() != ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    let parent = path.parent().unwrap();
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| Error::Io(error.error))?;
    File::open(parent)?.sync_all()?;
    Ok(())
}
