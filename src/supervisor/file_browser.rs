//! POSIX descriptor-anchored observations; exclusive publication remains Linux-only.
use super::{filesystem, protocol::SupervisorContext};
use crate::{
    DirectoryCursor, DirectoryEntry, DirectoryPage, Error, FileKind, FileRevision, ListDirectory,
    ReadVersionedFile, Result, VersionedFileChunk, WorkspacePath,
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
    path::Path,
};

#[cfg(target_os = "linux")]
use crate::{FilePrecondition, PublishFile, PublishOutcome};
#[cfg(target_os = "linux")]
use std::{
    ffi::CString,
    io::Write,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::PermissionsExt},
    },
};

const MAX_ENTRIES: usize = 4096;
fn checked_path(path: &WorkspacePath) -> Result<()> {
    if path.as_str().len() > 4096 {
        return Err(Error::InvalidInput(
            "browser path exceeds 4096 bytes".into(),
        ));
    }
    Ok(())
}
fn revision(
    context: &SupervisorContext,
    path: &WorkspacePath,
    metadata: &fs::Metadata,
) -> FileRevision {
    revision_values(
        context,
        path,
        &[
            metadata.dev(),
            metadata.ino(),
            metadata.len(),
            metadata.mode() as u64,
            metadata.mtime() as u64,
            metadata.mtime_nsec() as u64,
            metadata.ctime() as u64,
            metadata.ctime_nsec() as u64,
        ],
    )
}
fn revision_values(
    context: &SupervisorContext,
    path: &WorkspacePath,
    values: &[u64],
) -> FileRevision {
    let mut hash = Sha256::new();
    hash.update(context.workspace_id.as_str());
    hash.update([0]);
    hash.update(&context.root);
    hash.update([0]);
    hash.update(path.as_str());
    for value in values {
        hash.update(value.to_le_bytes());
    }
    FileRevision(format!("{:x}", hash.finalize()))
}
/// A listing is an observation, not a transactional tree snapshot. Entry tokens
/// include file metadata, so an in-place edit invalidates directory continuations.
pub(super) fn list(context: &SupervisorContext, request: ListDirectory) -> Result<DirectoryPage> {
    request.validate()?;
    let directory =
        filesystem::directory(&Path::new(&context.root).join(request.path.as_str()), false)?;
    let before = revision(context, &request.path, &directory.metadata()?);
    let mut entries = Vec::new();
    for (name, metadata) in super::directory_entries::entries(&directory, MAX_ENTRIES)? {
        let path = WorkspacePath::new(format!("{}/{}", request.path.as_str(), name))?;
        checked_path(&path)?;
        let kind = match metadata.st_mode & libc::S_IFMT {
            libc::S_IFREG => FileKind::File,
            libc::S_IFDIR => FileKind::Directory,
            libc::S_IFLNK => FileKind::Symlink,
            _ => FileKind::Other,
        };
        #[cfg(target_os = "linux")]
        let device = metadata.st_dev;
        #[cfg(target_os = "macos")]
        let device = metadata.st_dev as u64;
        let revision = revision_values(
            context,
            &path,
            &[
                device,
                metadata.st_ino,
                metadata.st_size as u64,
                metadata.st_mode as u64,
                metadata.st_mtime as u64,
                metadata.st_mtime_nsec as u64,
                metadata.st_ctime as u64,
                metadata.st_ctime_nsec as u64,
            ],
        );
        entries.push(DirectoryEntry {
            revision,
            path,
            kind,
            size: metadata.st_size as u64,
        });
    }
    if revision(context, &request.path, &directory.metadata()?) != before {
        return Err(Error::Conflict(
            "directory changed while listing; restart".into(),
        ));
    }
    entries.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
    let mut hash = Sha256::new();
    hash.update(&before.0);
    for entry in &entries {
        hash.update(&entry.revision.0);
    }
    let revision = FileRevision(format!("{:x}", hash.finalize()));
    let offset = match request.cursor {
        None => 0,
        Some(cursor) if cursor.revision == revision => cursor.offset as usize,
        Some(_) => {
            return Err(Error::Conflict(
                "directory continuation is stale; restart".into(),
            ))
        }
    };
    if offset > entries.len() {
        return Err(Error::InvalidInput(
            "directory cursor exceeds listing".into(),
        ));
    }
    let end = offset
        .saturating_add(request.limit as usize)
        .min(entries.len());
    let next = (end < entries.len()).then(|| DirectoryCursor {
        revision: revision.clone(),
        offset: end as u32,
    });
    Ok(DirectoryPage {
        entries: entries[offset..end].to_vec(),
        revision,
        next,
    })
}
pub(super) fn read(
    context: &SupervisorContext,
    request: ReadVersionedFile,
) -> Result<VersionedFileChunk> {
    request.validate()?;
    let mut file = filesystem::project_file(Path::new(&context.root), &request.path, false)?;
    let metadata = file.metadata()?;
    let observed = revision(context, &request.path, &metadata);
    if request
        .expected
        .as_ref()
        .is_some_and(|value| *value != observed)
    {
        return Err(Error::Conflict(
            "file revision changed; discard buffered chunks".into(),
        ));
    }
    if request.offset > metadata.len() {
        return Err(Error::InvalidInput(
            "read offset exceeds file length".into(),
        ));
    }
    file.seek(SeekFrom::Start(request.offset))?;
    let mut bytes = vec![0; request.limit as usize];
    let size = file.read(&mut bytes)?;
    bytes.truncate(size);
    let next_offset = request.offset + size as u64;
    // Reopen the pathname to detect atomic replacement, not just in-place writes.
    let current = filesystem::project_file(Path::new(&context.root), &request.path, false)?;
    if revision(context, &request.path, &file.metadata()?) != observed
        || revision(context, &request.path, &current.metadata()?) != observed
    {
        return Err(Error::Conflict(
            "file changed during read; discard buffered chunks".into(),
        ));
    }
    Ok(VersionedFileChunk {
        bytes,
        next_offset,
        eof: next_offset >= metadata.len(),
        revision: observed,
    })
}
#[cfg(target_os = "linux")]
pub(super) fn publish(context: &SupervisorContext, request: PublishFile) -> Result<PublishOutcome> {
    request.validate()?;
    if !matches!(request.expected, FilePrecondition::Missing) {
        return Err(Error::Unsupported("native revision replacement requires external writer fencing; use checkpoint review and quiescent apply".into()));
    }
    let relative = Path::new(request.path.as_str());
    let parent = filesystem::directory(
        &Path::new(&context.root).join(relative.parent().unwrap_or(Path::new(""))),
        false,
    )?;
    // Unnamed staging has no project-visible partial content and is reclaimed
    // by the kernel on cancellation/process death, before or after publication.
    let mut file = anonymous_file(&parent)?;
    file.write_all(&request.bytes)?;
    file.set_permissions(fs::Permissions::from_mode(if request.executable {
        0o755
    } else {
        0o644
    }))?;
    file.sync_all()?;
    let source =
        CString::new(format!("/proc/self/fd/{}", file.as_raw_fd())).expect("descriptor path");
    let name = CString::new(
        relative
            .file_name()
            .ok_or_else(|| Error::InvalidInput("file path required".into()))?
            .as_bytes(),
    )
    .map_err(|_| Error::InvalidInput("file name contains NUL".into()))?;
    // Following our own descriptor symlink avoids requiring CAP_DAC_READ_SEARCH
    // for linkat(AT_EMPTY_PATH). Destination link creation is exclusive.
    let linked = unsafe {
        libc::linkat(
            libc::AT_FDCWD,
            source.as_ptr(),
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::AT_SYMLINK_FOLLOW,
        )
    };
    if linked < 0 {
        let error = std::io::Error::last_os_error();
        return if error.kind() == std::io::ErrorKind::AlreadyExists {
            Ok(PublishOutcome::Conflict)
        } else {
            Err(Error::Io(error))
        };
    }
    if parent.sync_all().is_err() {
        return Ok(PublishOutcome::Unknown);
    }
    match file.metadata() {
        Ok(metadata) => Ok(PublishOutcome::Created {
            revision: revision(context, &request.path, &metadata),
        }),
        Err(_) => Ok(PublishOutcome::Unknown),
    }
}

#[cfg(target_os = "linux")]
fn anonymous_file(parent: &fs::File) -> Result<fs::File> {
    let dot = CString::new(".").expect("literal");
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            dot.as_ptr(),
            libc::O_TMPFILE | libc::O_RDWR | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(Error::Unsupported(
            "filesystem cannot create anonymous publication staging".into(),
        ));
    }
    Ok(unsafe { fs::File::from_raw_fd(fd) })
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::{
        process::Command,
        time::{Duration, Instant},
    };
    #[test]
    fn anonymous_stage_child() {
        let Some(root) = std::env::var_os("WORKSPACE_TEST_ANONYMOUS_STAGE") else {
            return;
        };
        let directory = filesystem::directory(Path::new(&root), false).unwrap();
        let mut file = anonymous_file(&directory).unwrap();
        file.write_all(b"synthetic unpublished bytes").unwrap();
        fs::write(
            std::env::var_os("WORKSPACE_TEST_STAGE_READY").unwrap(),
            b"ready",
        )
        .unwrap();
        std::thread::sleep(Duration::from_secs(10));
    }
    #[test]
    fn anonymous_staging_is_reclaimed_when_supervisor_process_is_killed() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().join("project");
        fs::create_dir(&root).unwrap();
        let ready = temp.path().join("ready");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "supervisor::file_browser::tests::anonymous_stage_child",
                "--nocapture",
            ])
            .env("WORKSPACE_TEST_ANONYMOUS_STAGE", &root)
            .env("WORKSPACE_TEST_STAGE_READY", &ready)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !ready.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let was_ready = ready.exists();
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(was_ready, "child failed to stage anonymous file");
        assert_eq!(
            fs::read_dir(root).unwrap().count(),
            0,
            "no visible staging survives SIGKILL"
        );
    }
}
