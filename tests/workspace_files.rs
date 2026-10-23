//! Isolated Linux SDK -> provider -> subprocess supervisor -> synthetic filesystem E2E.
#![cfg(target_os = "linux")]
use std::{fs, path::Path, sync::Arc};
use tempfile::TempDir;
use workspace_sdk::{
    infrastructure::FileStateRepository,
    providers::{LocalConfig, LocalProvider},
    *,
};

fn sdk(base: &Path) -> WorkspaceSdk {
    let mut config = LocalConfig::trusted_host(base.join("targets"));
    config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
    WorkspaceSdk::builder(Arc::new(
        FileStateRepository::new(base.join("state")).unwrap(),
    ))
    .provider(Arc::new(LocalProvider::new(config).unwrap()))
    .unwrap()
    .build()
    .unwrap()
}
async fn open(sdk: &WorkspaceSdk, id: &str) -> Workspace {
    sdk.open(
        &ProviderId::new("local").unwrap(),
        OpenRequest {
            workspace_id: WorkspaceId::new(id).unwrap(),
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
fn listing(limit: u32) -> ListDirectory {
    ListDirectory {
        path: WorkspacePath::root(),
        limit,
        cursor: None,
    }
}
fn read(value: &str, expected: Option<FileRevision>) -> ReadVersionedFile {
    ReadVersionedFile {
        path: path(value),
        offset: 0,
        limit: 3,
        expected,
    }
}
fn create(value: &str, bytes: &[u8]) -> PublishFile {
    PublishFile {
        path: path(value),
        bytes: bytes.to_vec(),
        expected: FilePrecondition::Missing,
        executable: false,
    }
}

#[tokio::test]
async fn paginated_listing_includes_empty_directories_and_binary_files() {
    let temp = TempDir::new().unwrap();
    let sdk = sdk(temp.path());
    let workspace = open(&sdk, "pages").await;
    let root = workspace.descriptor().root;
    fs::create_dir(Path::new(&root).join("empty")).unwrap();
    fs::write(Path::new(&root).join("binary"), [0, 255, 128, 10]).unwrap();
    let first = workspace.list_directory(listing(1)).await.unwrap();
    assert_eq!(first.entries[0].path, path("binary"));
    let second = workspace
        .list_directory(ListDirectory {
            cursor: first.next,
            ..listing(1)
        })
        .await
        .unwrap();
    assert_eq!(second.entries[0].kind, FileKind::Directory);
    assert!(second.next.is_none());
    let chunk = workspace
        .read_versioned_file(read("binary", None))
        .await
        .unwrap();
    assert_eq!(chunk.bytes, [0, 255, 128]);
    assert!(!chunk.eof);
    let rest = workspace
        .read_versioned_file(ReadVersionedFile {
            offset: 3,
            expected: Some(chunk.revision),
            ..read("binary", None)
        })
        .await
        .unwrap();
    assert_eq!(rest.bytes, [10]);
    assert!(rest.eof);
}
#[tokio::test]
async fn stale_chunk_and_atomic_external_replacement_are_conflicts() {
    let temp = TempDir::new().unwrap();
    let sdk = sdk(temp.path());
    let workspace = open(&sdk, "stale").await;
    let root = workspace.descriptor().root;
    let file = Path::new(&root).join("file");
    fs::write(&file, b"abcdef").unwrap();
    let initial = workspace
        .read_versioned_file(read("file", None))
        .await
        .unwrap();
    fs::write(&file, b"human!").unwrap();
    assert!(matches!(
        workspace
            .read_versioned_file(read("file", Some(initial.revision)))
            .await,
        Err(Error::Conflict(_))
    ));
    let second = workspace
        .read_versioned_file(read("file", None))
        .await
        .unwrap();
    let replacement = Path::new(&root).join("replacement");
    fs::write(&replacement, b"agent!").unwrap();
    fs::rename(replacement, &file).unwrap();
    assert!(matches!(
        workspace
            .read_versioned_file(read("file", Some(second.revision)))
            .await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(fs::read(file).unwrap(), b"agent!");
}
#[tokio::test]
async fn changed_directory_pages_must_restart_including_in_place_edits() {
    let temp = TempDir::new().unwrap();
    let sdk = sdk(temp.path());
    let workspace = open(&sdk, "cursor").await;
    let root = workspace.descriptor().root;
    fs::write(Path::new(&root).join("a"), b"before").unwrap();
    fs::write(Path::new(&root).join("b"), b"x").unwrap();
    let first = workspace.list_directory(listing(1)).await.unwrap();
    fs::write(Path::new(&root).join("a"), b"after").unwrap();
    assert!(matches!(
        workspace
            .list_directory(ListDirectory {
                cursor: first.next,
                ..listing(1)
            })
            .await,
        Err(Error::Conflict(_))
    ));
}
#[tokio::test]
async fn no_clobber_create_is_atomic_under_simultaneous_sdk_participants() {
    let temp = TempDir::new().unwrap();
    let sdk = sdk(temp.path());
    let workspace = open(&sdk, "writers").await;
    let (human, agent) = tokio::join!(
        workspace.publish_file(create("shared", b"human")),
        workspace.publish_file(create("shared", b"agent"))
    );
    assert!(matches!(
        (&human, &agent),
        (
            Ok(PublishOutcome::Created { .. }),
            Ok(PublishOutcome::Conflict)
        ) | (
            Ok(PublishOutcome::Conflict),
            Ok(PublishOutcome::Created { .. })
        )
    ));
    let bytes = fs::read(Path::new(&workspace.descriptor().root).join("shared")).unwrap();
    assert!(bytes == b"human" || bytes == b"agent");
    assert_eq!(
        fs::read_dir(workspace.descriptor().root).unwrap().count(),
        1
    );
}
#[tokio::test]
async fn unmanaged_human_creation_is_preserved_and_replace_is_explicitly_unsupported() {
    let temp = TempDir::new().unwrap();
    let sdk = sdk(temp.path());
    let workspace = open(&sdk, "human").await;
    let file = Path::new(&workspace.descriptor().root).join("file");
    fs::write(&file, b"human text").unwrap();
    assert!(matches!(
        workspace
            .publish_file(create("file", b"agent text"))
            .await
            .unwrap(),
        PublishOutcome::Conflict
    ));
    let revision = workspace
        .read_versioned_file(read("file", None))
        .await
        .unwrap()
        .revision;
    assert!(matches!(
        workspace
            .publish_file(PublishFile {
                expected: FilePrecondition::Revision(revision),
                ..create("file", b"agent text")
            })
            .await,
        Err(Error::Unsupported(_))
    ));
    assert_eq!(fs::read(file).unwrap(), b"human text");
}
#[tokio::test]
async fn observer_reconciles_external_create_edit_rename_delete_and_restart() {
    let temp = TempDir::new().unwrap();
    let first_sdk = sdk(temp.path());
    let workspace = open(&first_sdk, "observe").await;
    let root = workspace.descriptor().root;
    let mut observer = workspace.observe_directory(WorkspacePath::root());
    assert!(observer.poll().await.unwrap().reset);
    fs::write(Path::new(&root).join("file"), b"initial").unwrap();
    assert_eq!(observer.poll().await.unwrap().upserted.len(), 1);
    assert!(observer.poll().await.unwrap().upserted.is_empty());
    fs::write(Path::new(&root).join("file"), b"edited!").unwrap();
    assert_eq!(observer.poll().await.unwrap().upserted.len(), 1);
    fs::rename(
        Path::new(&root).join("file"),
        Path::new(&root).join("renamed"),
    )
    .unwrap();
    let change = observer.poll().await.unwrap();
    assert_eq!(change.removed, vec![path("file")]);
    assert_eq!(change.upserted[0].path, path("renamed"));
    drop(observer);
    drop(workspace);
    drop(first_sdk);
    let second_sdk = sdk(temp.path());
    let workspace = second_sdk
        .workspace(&WorkspaceId::new("observe").unwrap())
        .await
        .unwrap();
    let mut observer = workspace.observe_directory(WorkspacePath::root());
    let reset = observer.poll().await.unwrap();
    assert!(reset.reset);
    assert_eq!(reset.upserted.len(), 1);
    fs::remove_file(Path::new(&root).join("renamed")).unwrap();
    assert_eq!(
        observer.poll().await.unwrap().removed,
        vec![path("renamed")]
    );
}
#[tokio::test]
async fn failed_observation_does_not_erase_baseline_and_drop_leaves_no_resources() {
    let temp = TempDir::new().unwrap();
    let sdk = sdk(temp.path());
    let workspace = open(&sdk, "failure").await;
    let root = workspace.descriptor().root;
    let dir = Path::new(&root).join("dir");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("file"), b"data").unwrap();
    let mut observer = workspace.observe_directory(path("dir"));
    observer.poll().await.unwrap();
    fs::rename(&dir, Path::new(&root).join("moved")).unwrap();
    assert!(observer.poll().await.is_err());
    fs::rename(Path::new(&root).join("moved"), &dir).unwrap();
    fs::remove_file(dir.join("file")).unwrap();
    let recovered = observer.poll().await.unwrap();
    assert!(!recovered.reset);
    assert_eq!(recovered.removed, vec![path("dir/file")]);
    let entries = fs::read_dir(&root).unwrap().count();
    drop(observer);
    assert_eq!(fs::read_dir(&root).unwrap().count(), entries);
}
#[tokio::test]
async fn symlinks_special_files_wrong_revision_and_control_state_are_isolated() {
    use std::os::unix::fs::symlink;
    let temp = TempDir::new().unwrap();
    let sdk = sdk(temp.path());
    let workspace = open(&sdk, "bounds").await;
    let root = workspace.descriptor().root;
    let outside = temp.path().join("outside");
    fs::write(&outside, b"private").unwrap();
    symlink(&outside, Path::new(&root).join("link")).unwrap();
    symlink(temp.path(), Path::new(&root).join("escape")).unwrap();
    assert!(workspace
        .read_versioned_file(read("link", None))
        .await
        .is_err());
    assert!(workspace
        .list_directory(ListDirectory {
            path: path("escape"),
            ..listing(8)
        })
        .await
        .is_err());
    assert!(workspace
        .publish_file(create("escape/outside", b"clobber"))
        .await
        .is_err());
    assert!(matches!(
        workspace
            .publish_file(create("link", b"clobber"))
            .await
            .unwrap(),
        PublishOutcome::Conflict
    ));
    let page = workspace.list_directory(listing(256)).await.unwrap();
    assert!(page
        .entries
        .iter()
        .all(|entry| entry.kind == FileKind::Symlink));
    let fifo = std::ffi::CString::new(Path::new(&root).join("fifo").to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    assert!(workspace
        .read_versioned_file(read("fifo", None))
        .await
        .is_err());
    assert!(workspace
        .list_directory(listing(256))
        .await
        .unwrap()
        .entries
        .iter()
        .any(|entry| entry.kind == FileKind::Other));
    assert_eq!(fs::read(outside).unwrap(), b"private");
    assert!(WorkspacePath::new("../control/owner.json").is_err());
    fs::write(Path::new(&root).join("a"), b"same").unwrap();
    fs::write(Path::new(&root).join("b"), b"same").unwrap();
    let revision = workspace
        .read_versioned_file(read("a", None))
        .await
        .unwrap()
        .revision;
    assert!(matches!(
        workspace
            .read_versioned_file(read("b", Some(revision)))
            .await,
        Err(Error::Conflict(_))
    ));
}
#[tokio::test]
async fn bounds_are_enforced_for_reads_pages_publication_and_large_directories() {
    let temp = TempDir::new().unwrap();
    let sdk = sdk(temp.path());
    let workspace = open(&sdk, "limits").await;
    for limit in [0, 257] {
        assert!(workspace.list_directory(listing(limit)).await.is_err());
    }
    assert!(workspace
        .publish_file(create("large", &vec![0; 262145]))
        .await
        .is_err());
    assert!(!Path::new(&workspace.descriptor().root)
        .join("large")
        .exists());
    for limit in [0, 262145] {
        assert!(workspace
            .read_versioned_file(ReadVersionedFile {
                limit,
                ..read("missing", None)
            })
            .await
            .is_err());
    }
    let root = workspace.descriptor().root;
    for index in 0..4097 {
        fs::write(Path::new(&root).join(index.to_string()), []).unwrap();
    }
    assert!(matches!(
        workspace.list_directory(listing(256)).await,
        Err(Error::Unsupported(_))
    ));
}
#[tokio::test]
async fn large_file_is_read_in_bounded_chunks_without_mixing_revisions() {
    let temp = TempDir::new().unwrap();
    let sdk = sdk(temp.path());
    let workspace = open(&sdk, "large").await;
    let bytes = vec![0x9a; 1024 * 1024 + 7];
    fs::write(
        Path::new(&workspace.descriptor().root).join("large"),
        &bytes,
    )
    .unwrap();
    let mut offset = 0;
    let mut expected = None;
    let mut result = Vec::new();
    loop {
        let chunk = workspace
            .read_versioned_file(ReadVersionedFile {
                offset,
                limit: 262144,
                expected: expected.clone(),
                ..read("large", None)
            })
            .await
            .unwrap();
        expected = Some(chunk.revision);
        offset = chunk.next_offset;
        result.extend(chunk.bytes);
        if chunk.eof {
            break;
        }
    }
    assert_eq!(result, bytes);
}

#[tokio::test]
#[ignore = "manual local performance qualification, synthetic data only"]
async fn file_browser_performance_samples() {
    let temp = TempDir::new().unwrap();
    let sdk = sdk(temp.path());
    let workspace = open(&sdk, "performance").await;
    let root = workspace.descriptor().root;
    for index in 0..4096 {
        fs::write(Path::new(&root).join(format!("{index:04}")), []).unwrap();
    }
    for sample in 0..3 {
        let start = std::time::Instant::now();
        let mut observer = workspace.observe_directory(WorkspacePath::root());
        assert_eq!(observer.poll().await.unwrap().upserted.len(), 4096);
        let first = start.elapsed();
        let repeat = std::time::Instant::now();
        assert!(observer.poll().await.unwrap().upserted.is_empty());
        println!(
            "files_performance sample={sample} entries=4096 initial_ms={} unchanged_ms={}",
            first.as_millis(),
            repeat.elapsed().as_millis()
        );
    }
}
