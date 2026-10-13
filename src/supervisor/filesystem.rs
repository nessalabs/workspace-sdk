use super::protocol::{SupervisorContext, TransferredFileChunk, WorkingDirectory};
use crate::{Error, Ownership, Result, WorkspaceId, WorkspacePath};
use fs2::FileExt;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    ffi::{CString, OsStr},
    fs::{self, File, OpenOptions},
    io::{Error as IoError, ErrorKind, Read, Seek, SeekFrom, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        },
    },
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;
pub const MAX_CHUNK: usize = 256 * 1024;
#[derive(Serialize, Deserialize)]
struct OwnerRecord {
    version: u32,
    workspace_id: WorkspaceId,
    root: String,
    control: String,
    ownership: Ownership,
}
fn invalid(message: &str) -> Error {
    Error::InvalidInput(message.into())
}
pub fn absolute(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path == Path::new("/")
        || path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::CurDir | Component::Prefix(_)
            )
        })
    {
        return Err(invalid("expected an absolute normalized POSIX directory"));
    }
    Ok(())
}
fn cstring(value: &OsStr) -> Result<CString> {
    CString::new(value.as_bytes()).map_err(|_| invalid("path contains NUL"))
}
/// Traverse every directory component with O_NOFOLLOW, including root ancestors.
pub fn directory(path: &Path, create: bool) -> Result<File> {
    absolute(path)?;
    let mut parent = File::open("/")?;
    for part in path.components() {
        let Component::Normal(part) = part else {
            continue;
        };
        let name = cstring(part)?;
        if create {
            let result = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
            if result < 0 {
                let e = IoError::last_os_error();
                if e.kind() != ErrorKind::AlreadyExists {
                    return Err(e.into());
                }
            }
        }
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(IoError::last_os_error().into());
        }
        parent = unsafe { File::from_raw_fd(fd) };
    }
    Ok(parent)
}
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    if file.metadata()?.len() > 32 * 1024 * 1024 {
        return Err(Error::State("private state file exceeds size limit".into()));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    serde_json::from_slice(&bytes)
        .map_err(|_| Error::State("private state record is malformed".into()))
}
pub fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("file needs a parent directory"))?;
    let mut pending = tempfile::NamedTempFile::new_in(parent)?;
    pending.write_all(bytes)?;
    pending.as_file().sync_all()?;
    pending.persist(path).map_err(|e| Error::Io(e.error))?;
    File::open(parent)?.sync_all()?;
    Ok(())
}
pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    atomic(path, &serde_json::to_vec(value)?)
}
pub fn lock(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    file.lock_exclusive()?;
    Ok(file)
}
pub fn live_lock(path: &Path) -> Result<bool> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(false),
        Err(e) if e.kind() == ErrorKind::WouldBlock => Ok(true),
        Err(e) => Err(e.into()),
    }
}
pub fn initialize(context: &SupervisorContext, create: bool) -> Result<()> {
    let root = Path::new(&context.root);
    let control = Path::new(&context.control);
    absolute(root)?;
    absolute(control)?;
    if root.starts_with(control) || control.starts_with(root) {
        return Err(invalid(
            "project and private control roots must be separate directories",
        ));
    }
    // Reject a conflicting existing namespace before changing directories,
    // permissions, or lock files named by the rejected request.
    match directory(control, false) {
        Ok(_) => match fs::symlink_metadata(control.join("owner.json")) {
            Ok(_) => check(context)?,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                if fs::read_dir(control)?.next().transpose()?.is_some() {
                    return Err(Error::Conflict(
                        "unowned control directory is not empty".into(),
                    ));
                }
            }
            Err(error) => return Err(error.into()),
        },
        Err(Error::Io(error)) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    directory(control, true)?;
    let _guard = lock(&control.join("workspace.lock"))?;
    let marker = control.join("owner.json");
    match fs::symlink_metadata(&marker) {
        Ok(_) => check(context)?,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            directory(root, create)?;
            fs::set_permissions(control, fs::Permissions::from_mode(0o700))?;
            write_json(
                &marker,
                &OwnerRecord {
                    version: 1,
                    workspace_id: context.workspace_id.clone(),
                    root: context.root.clone(),
                    control: context.control.clone(),
                    ownership: context.ownership.clone(),
                },
            )?;
        }
        Err(error) => return Err(error.into()),
    }
    directory(&control.join("runs"), true)?;
    directory(&control.join("requests"), true)?;
    Ok(())
}
pub fn check(context: &SupervisorContext) -> Result<()> {
    directory(Path::new(&context.control), false)?;
    let owner: OwnerRecord = read_json(&Path::new(&context.control).join("owner.json"))?;
    if owner.version != 1
        || owner.workspace_id != context.workspace_id
        || owner.root != context.root
        || owner.control != context.control
        || owner.ownership != context.ownership
    {
        return Err(Error::Conflict(
            "workspace owner marker does not match".into(),
        ));
    }
    directory(Path::new(&context.root), false)?;
    Ok(())
}
fn private_requests(control: &Path) -> Result<File> {
    let requests = directory(&control.join("requests"), false)?;
    let metadata = requests.metadata()?;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        return Err(invalid(
            "requests directory must be private and owned by the current user",
        ));
    }
    Ok(requests)
}
pub fn prepare_request(context: &SupervisorContext, id: Uuid) -> Result<()> {
    let requests = private_requests(Path::new(&context.control))?;
    let name = cstring(OsStr::new(&format!("{id}.json")))?;
    let fd = unsafe {
        libc::openat(
            requests.as_raw_fd(),
            name.as_ptr(),
            libc::O_CREAT | libc::O_EXCL | libc::O_WRONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(IoError::last_os_error().into());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    file.sync_all()?;
    requests.sync_all()?;
    Ok(())
}
pub fn remove_request(context: &SupervisorContext, id: Uuid) -> Result<()> {
    let requests = private_requests(Path::new(&context.control))?;
    let name = cstring(OsStr::new(&format!("{id}.json")))?;
    let result = unsafe { libc::unlinkat(requests.as_raw_fd(), name.as_ptr(), 0) };
    if result < 0 {
        let error = IoError::last_os_error();
        if error.kind() != ErrorKind::NotFound {
            return Err(error.into());
        }
    }
    Ok(())
}
/// Consume only a private UUID request in a verified SDK control directory.
pub fn open_request(path: &Path) -> Result<(File, SupervisorContext)> {
    absolute(path)?;
    let requests = path
        .parent()
        .ok_or_else(|| invalid("request file needs a parent"))?;
    if requests.file_name() != Some(OsStr::new("requests")) {
        return Err(invalid(
            "request file must be inside the private requests directory",
        ));
    }
    let control = requests
        .parent()
        .ok_or_else(|| invalid("request file needs a control root"))?;
    directory(control, false)?;
    let owner: OwnerRecord = read_json(&control.join("owner.json"))?;
    if owner.version != 1 || Path::new(&owner.control) != control {
        return Err(Error::Conflict(
            "request file control owner does not match".into(),
        ));
    }
    let context = SupervisorContext {
        workspace_id: owner.workspace_id,
        root: owner.root,
        control: owner.control,
        ownership: owner.ownership,
    };
    check(&context)?;
    let directory = private_requests(control)?;
    let filename = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| invalid("request filename must be a UUID"))?;
    let id = filename
        .strip_suffix(".json")
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| invalid("request filename must be a UUID"))?;
    if filename != format!("{id}.json") {
        return Err(invalid("request filename must use its canonical UUID"));
    }
    let name = cstring(OsStr::new(filename))?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err(IoError::last_os_error().into());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(invalid(
            "request file must be a private regular file owned by the current user",
        ));
    }
    if unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } < 0 {
        return Err(IoError::last_os_error().into());
    }
    Ok((file, context))
}
pub fn working_directory(
    context: &SupervisorContext,
    directory_scope: &WorkingDirectory,
) -> Result<PathBuf> {
    let (root, path) = match directory_scope {
        WorkingDirectory::Project(path) => (&context.root, path),
        WorkingDirectory::Control(path) => (&context.control, path),
    };
    let full = Path::new(root).join(path.as_str());
    directory(&full, false)?;
    Ok(full)
}
fn project_file(root: &Path, path: &WorkspacePath, write: bool) -> Result<File> {
    let file_path = Path::new(path.as_str());
    let name = file_path
        .file_name()
        .ok_or_else(|| invalid("a file path is required"))?;
    if path.as_str() == "." {
        return Err(invalid("a file path is required"));
    }
    let parent_path = root.join(file_path.parent().unwrap_or(Path::new("")));
    let parent = directory(&parent_path, write)?;
    let name = cstring(name)?;
    let flags = if write {
        libc::O_WRONLY | libc::O_CREAT
    } else {
        libc::O_RDONLY
    };
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            0o600,
        )
    };
    if fd < 0 {
        return Err(IoError::last_os_error().into());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    if !file.metadata()?.is_file() {
        return Err(invalid("only regular files can be transferred"));
    }
    Ok(file)
}
pub fn read_file(
    root: &Path,
    path: &WorkspacePath,
    offset: u64,
    limit: u32,
) -> Result<TransferredFileChunk> {
    if limit == 0 || limit as usize > MAX_CHUNK {
        return Err(invalid("file read limit must be 1..=262144 bytes"));
    }
    let mut file = project_file(root, path, false)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; limit as usize];
    let size = file.read(&mut bytes)?;
    bytes.truncate(size);
    let mut next = [0; 1];
    let eof = file.read(&mut next)? == 0;
    let next_offset = offset
        .checked_add(size as u64)
        .ok_or_else(|| invalid("file offset overflow"))?;
    Ok(TransferredFileChunk {
        bytes,
        next_offset,
        eof,
    })
}
pub fn write_file(
    root: &Path,
    path: &WorkspacePath,
    offset: u64,
    bytes: &[u8],
    truncate: bool,
    executable: bool,
) -> Result<()> {
    if bytes.len() > MAX_CHUNK {
        return Err(invalid("file write chunk exceeds 256 KiB"));
    }
    let mut file = project_file(root, path, true)?;
    if truncate {
        file.set_len(0)?;
    }
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    file.set_permissions(fs::Permissions::from_mode(if executable {
        0o755
    } else {
        0o644
    }))?;
    Ok(())
}
