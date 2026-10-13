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
