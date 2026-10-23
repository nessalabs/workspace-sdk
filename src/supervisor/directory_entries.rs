//! Descriptor-relative POSIX directory enumeration, including symlinks without following them.
use crate::{Error, Result};
use std::{
    ffi::{CStr, CString},
    fs::File,
    io,
    mem::MaybeUninit,
    os::fd::{AsRawFd, IntoRawFd},
};

struct Directory(*mut libc::DIR);
impl Drop for Directory {
    fn drop(&mut self) {
        unsafe {
            libc::closedir(self.0);
        }
    }
}
fn errno() -> *mut libc::c_int {
    #[cfg(target_os = "linux")]
    unsafe {
        libc::__errno_location()
    }
    #[cfg(target_os = "macos")]
    unsafe {
        libc::__error()
    }
}
/// At most `limit` entries are retained. The directory stream owns and closes its duplicated fd.
pub(super) fn entries(directory: &File, limit: usize) -> Result<Vec<(String, libc::stat)>> {
    let fd = directory.try_clone()?.into_raw_fd();
    let raw = unsafe { libc::fdopendir(fd) };
    if raw.is_null() {
        let error = io::Error::last_os_error();
        unsafe {
            libc::close(fd);
        }
        return Err(error.into());
    }
    let stream = Directory(raw);
    let mut result = Vec::new();
    loop {
        unsafe {
            *errno() = 0;
        }
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            let error = unsafe { *errno() };
            return if error == 0 {
                Ok(result)
            } else {
                Err(io::Error::from_raw_os_error(error).into())
            };
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        if result.len() == limit {
            return Err(Error::Unsupported(
                "directory exceeds native 4096-entry scan bound".into(),
            ));
        }
        let text = name
            .to_str()
            .map_err(|_| Error::Unsupported("non-UTF-8 directory entry".into()))?
            .to_owned();
        let name = CString::new(name.to_bytes()).expect("directory entry is NUL terminated");
        let mut metadata = MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                directory.as_raw_fd(),
                name.as_ptr(),
                metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(io::Error::last_os_error().into());
        }
        result.push((text, unsafe { metadata.assume_init() }));
    }
}
