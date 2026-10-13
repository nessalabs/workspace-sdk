use crate::{Error, Result};
use std::ffi::{CString, OsStr};
use std::fs::File;
use std::io::{Error as IoError, Write};
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

/// Create a private runtime directory using directory descriptors throughout.
/// Existing ancestors and the final directory may not be symbolic links.
pub(crate) fn ensure_private_directory(control_root: &Path, path: &Path) -> Result<()> {
    validate_private_path(control_root, path)?;
    let directory = open_private_directory(control_root, path)?;
    private_mode(&directory, 0o700)?;
    Ok(())
}

/// Atomically replace a private regular file beneath the control directory.
/// Content never appears in command arguments, filenames, or error messages.
pub(crate) fn write_private_file(
    control_root: &Path,
    path: &Path,
    bytes: &[u8],
    mode: u32,
) -> Result<()> {
    validate_private_path(control_root, path)?;
    if bytes.len() > 65_536 || !matches!(mode, 0o600 | 0o700) {
        return Err(Error::InvalidInput(
            "private runtime files require at most 64 KiB and owner-only permissions".into(),
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| Error::InvalidInput("private runtime file has no parent".into()))?;
    let directory = open_private_directory(control_root, parent)?;
    private_mode(&directory, 0o700)?;
    let filename = c_string(
        path.file_name()
            .ok_or_else(|| Error::InvalidInput("private runtime file has no name".into()))?,
    )?;
    ensure_regular_or_absent(&directory, &filename)?;
    let temporary =
        CString::new(format!(".{}.tmp", Uuid::new_v4())).expect("UUID filenames have no NUL bytes");
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            temporary.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            mode as libc::c_uint,
        )
    };
    if fd < 0 {
        return Err(IoError::last_os_error().into());
    }
    let mut output = unsafe { File::from_raw_fd(fd) };
    let result = (|| -> Result<()> {
        output.write_all(bytes)?;
        output.sync_all()?;
        // Recheck the destination without following links immediately before
        // replacement. renameat itself never follows a destination symlink.
        ensure_regular_or_absent(&directory, &filename)?;
        if unsafe {
            libc::renameat(
                directory.as_raw_fd(),
                temporary.as_ptr(),
                directory.as_raw_fd(),
                filename.as_ptr(),
            )
        } != 0
        {
            return Err(IoError::last_os_error().into());
        }
        if unsafe { libc::fsync(directory.as_raw_fd()) } != 0 {
            return Err(IoError::last_os_error().into());
        }
        Ok(())
    })();
    if result.is_err() {
        // Best-effort removal is safe even when rename succeeded: the temporary
        // name no longer exists and the committed destination is left intact.
        unsafe { libc::unlinkat(directory.as_raw_fd(), temporary.as_ptr(), 0) };
    }
    result
}

fn validate_private_path(control_root: &Path, path: &Path) -> Result<()> {
    for candidate in [control_root, path] {
        if !candidate.is_absolute()
            || candidate.as_os_str().as_bytes().contains(&0)
            || candidate
                .components()
                .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
        {
            return Err(Error::InvalidInput(
                "private runtime paths must be normalized absolute paths".into(),
            ));
        }
    }
    if control_root == Path::new("/") || path == control_root || !path.starts_with(control_root) {
        return Err(Error::InvalidInput(
            "private runtime path must be beneath the workspace control directory".into(),
        ));
    }
    Ok(())
}

fn open_private_directory(control_root: &Path, path: &Path) -> Result<OwnedFd> {
    let fd = unsafe {
        libc::open(
            c"/".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(IoError::last_os_error().into());
    }
    let mut directory = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut walked = PathBuf::from("/");
    for component in path.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        walked.push(name);
        let name = c_string(name)?;
        if walked != control_root && walked.starts_with(control_root) {
            let created = unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) };
            if created != 0 {
                let error = IoError::last_os_error();
                if error.raw_os_error() != Some(libc::EEXIST) {
                    return Err(error.into());
                }
            }
        }
        let following = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if following < 0 {
            return Err(IoError::last_os_error().into());
        }
        directory = unsafe { OwnedFd::from_raw_fd(following) };
    }
    Ok(directory)
}

fn ensure_regular_or_absent(directory: &OwnedFd, name: &CString) -> Result<()> {
    let mut metadata = MaybeUninit::<libc::stat>::uninit();
    let result = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == 0 {
        let metadata = unsafe { metadata.assume_init() };
        if metadata.st_mode & libc::S_IFMT != libc::S_IFREG {
            return Err(Error::InvalidInput(
                "private runtime file must be a regular file, not a link or directory".into(),
            ));
        }
    } else {
        let error = IoError::last_os_error();
        if error.raw_os_error() != Some(libc::ENOENT) {
            return Err(error.into());
        }
    }
    Ok(())
}

fn private_mode(directory: &OwnedFd, mode: libc::mode_t) -> Result<()> {
    if unsafe { libc::fchmod(directory.as_raw_fd(), mode) } != 0 {
        return Err(IoError::last_os_error().into());
    }
    Ok(())
}

fn c_string(value: &OsStr) -> Result<CString> {
    CString::new(value.as_bytes())
        .map_err(|_| Error::InvalidInput("private runtime path contains NUL".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt};

    #[test]
    fn private_files_are_atomic_and_reject_link_traversal() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let control = root.join("control");
        fs::create_dir(&control).unwrap();
        let auth = control.join("runtime/auth");
        ensure_private_directory(&control, &auth).unwrap();
        let cache = auth.join("auth.json");
        write_private_file(&control, &cache, b"first", 0o600).unwrap();
        write_private_file(&control, &cache, b"second", 0o600).unwrap();
        assert_eq!(fs::read(&cache).unwrap(), b"second");
        assert_eq!(
            fs::metadata(&auth).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&cache).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(fs::read_dir(&auth).unwrap().count(), 1);

        let outside = root.join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, control.join("link")).unwrap();
        assert!(write_private_file(&control, &control.join("link/escaped"), b"no", 0o600).is_err());
        assert!(!outside.join("escaped").exists());
        symlink(&cache, auth.join("linked-cache")).unwrap();
        assert!(write_private_file(&control, &auth.join("linked-cache"), b"no", 0o600).is_err());
        assert_eq!(fs::read(&cache).unwrap(), b"second");
        assert!(ensure_private_directory(&control, &outside).is_err());
    }
}
