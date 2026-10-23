//! The same native subprocess observations run in Linux and macOS CI.
#![cfg(any(target_os = "linux", target_os = "macos"))]
use std::{fs, path::Path, sync::Arc};
use tempfile::TempDir;
use workspace_sdk::{
    infrastructure::FileStateRepository,
    providers::{LocalConfig, LocalProvider},
    *,
};
async fn setup(temp: &TempDir) -> Workspace {
    let mut config = LocalConfig::trusted_host(temp.path().join("targets"));
    config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
    let sdk = WorkspaceSdk::builder(Arc::new(
        FileStateRepository::new(temp.path().join("state")).unwrap(),
    ))
    .provider(Arc::new(LocalProvider::new(config).unwrap()))
    .unwrap()
    .build()
    .unwrap();
    sdk.open(
        &ProviderId::new("local").unwrap(),
        OpenRequest {
            workspace_id: WorkspaceId::new("portable").unwrap(),
            config: ProviderOptions::default(),
            resources: ResourceRequirements::default(),
        },
    )
    .await
    .unwrap()
}
fn path(value: &str) -> WorkspacePath {
    WorkspacePath::new(value).unwrap()
}
fn list(limit: u32) -> ListDirectory {
    ListDirectory {
        path: WorkspacePath::root(),
        limit,
        cursor: None,
    }
}
fn read(expected: Option<FileRevision>) -> ReadVersionedFile {
    ReadVersionedFile {
        path: path("file"),
        offset: 0,
        limit: 4,
        expected,
    }
}
#[tokio::test]
async fn portable_pages_read_tokens_and_external_reconciliation() {
    let temp = TempDir::new().unwrap();
    let workspace = setup(&temp).await;
    let root = workspace.descriptor().root;
    fs::write(Path::new(&root).join("file"), [0, 255, 128, 1, 2]).unwrap();
    fs::create_dir(Path::new(&root).join("empty")).unwrap();
    let first = workspace.list_directory(list(1)).await.unwrap();
    assert_eq!(first.entries[0].path, path("empty"));
    let second = workspace
        .list_directory(ListDirectory {
            cursor: first.next,
            ..list(1)
        })
        .await
        .unwrap();
    assert_eq!(second.entries[0].path, path("file"));
    assert!(second.next.is_none());
    let chunk = workspace.read_versioned_file(read(None)).await.unwrap();
    assert_eq!(chunk.bytes, [0, 255, 128, 1]);
    assert_eq!(
        chunk.revision, second.entries[0].revision,
        "listing and reading name the same observation"
    );
    let mut observer = workspace.observe_directory(WorkspacePath::root());
    assert!(observer.poll().await.unwrap().reset);
    fs::write(Path::new(&root).join("file"), b"human").unwrap();
    assert!(matches!(
        workspace
            .read_versioned_file(read(Some(chunk.revision)))
            .await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(observer.poll().await.unwrap().upserted.len(), 1);
    fs::rename(
        Path::new(&root).join("file"),
        Path::new(&root).join("renamed"),
    )
    .unwrap();
    let change = observer.poll().await.unwrap();
    assert_eq!(change.removed, vec![path("file")]);
    assert_eq!(change.upserted[0].path, path("renamed"));
}
#[tokio::test]
async fn portable_descriptor_listing_never_follows_symlinks_or_special_files() {
    use std::os::unix::fs::symlink;
    let temp = TempDir::new().unwrap();
    let workspace = setup(&temp).await;
    let root = workspace.descriptor().root;
    let outside = temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("private"), b"unread").unwrap();
    symlink(&outside, Path::new(&root).join("escape")).unwrap();
    symlink(outside.join("private"), Path::new(&root).join("file")).unwrap();
    let page = workspace.list_directory(list(8)).await.unwrap();
    assert!(page
        .entries
        .iter()
        .all(|entry| entry.kind == FileKind::Symlink));
    assert!(workspace
        .list_directory(ListDirectory {
            path: path("escape"),
            ..list(8)
        })
        .await
        .is_err());
    assert!(workspace.read_versioned_file(read(None)).await.is_err());
    fs::remove_file(Path::new(&root).join("file")).unwrap();
    let fifo = std::ffi::CString::new(Path::new(&root).join("file").to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    assert!(workspace.read_versioned_file(read(None)).await.is_err());
    assert!(workspace
        .list_directory(list(8))
        .await
        .unwrap()
        .entries
        .iter()
        .any(|entry| entry.kind == FileKind::Other));
}
#[tokio::test]
async fn portable_repeated_streams_close_and_nonportable_names_reject() {
    use std::os::unix::ffi::OsStringExt;
    let temp = TempDir::new().unwrap();
    let workspace = setup(&temp).await;
    let root = workspace.descriptor().root;
    for index in 0..20 {
        fs::write(Path::new(&root).join(index.to_string()), []).unwrap();
    }
    for _ in 0..32 {
        assert_eq!(
            workspace
                .list_directory(list(256))
                .await
                .unwrap()
                .entries
                .len(),
            20
        );
    }
    fs::write(
        Path::new(&root).join(std::ffi::OsString::from_vec(vec![0xff])),
        [],
    )
    .unwrap();
    assert!(matches!(
        workspace.list_directory(list(256)).await,
        Err(Error::Unsupported(_))
    ));
}
#[cfg(target_os = "macos")]
#[tokio::test]
async fn macos_publication_remains_explicitly_unsupported() {
    let temp = TempDir::new().unwrap();
    let workspace = setup(&temp).await;
    assert!(matches!(
        workspace
            .publish_file(PublishFile {
                path: path("new"),
                bytes: b"safe".to_vec(),
                expected: FilePrecondition::Missing,
                executable: false
            })
            .await,
        Err(Error::Unsupported(_))
    ));
    assert!(!Path::new(&workspace.descriptor().root).join("new").exists());
}
