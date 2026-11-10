use super::*;
use crate::checkpoint::domain::FileEntry;
use crate::checkpoint::infrastructure::archive::encode_tree;
use crate::checkpoint::infrastructure::filesystem::PublicationStep;
use std::collections::BTreeMap;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use tempfile::{tempdir, TempDir};

struct ImportFixture {
    _directory: TempDir,
    root: PathBuf,
    archive: PathBuf,
}

impl ImportFixture {
    fn new(entries: &[(&str, EntryKind, &[u8])]) -> Self {
        let directory = tempdir().unwrap();
        let root = fs::canonicalize(directory.path()).unwrap().join("project");
        let staging = root.join(".workspace-sdk/staging");
        fs::create_dir_all(&staging).unwrap();
        let tree = Tree {
            files: entries
                .iter()
                .map(|(path, kind, bytes)| {
                    let entry = FileEntry {
                        path: (*path).into(),
                        kind: *kind,
                        mode: if *kind == EntryKind::Symlink {
                            120000
                        } else {
                            100644
                        },
                        size: bytes.len() as u64,
                        sha256: digest(bytes),
                    };
                    (
                        (*path).into(),
                        TreeFile {
                            entry,
                            bytes: bytes.to_vec(),
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>(),
            excluded: vec![],
        };
        let archive = staging.join("tree.json");
        fs::write(
            &archive,
            encode_tree(&tree, &CheckpointLimits::default()).unwrap(),
        )
        .unwrap();
        Self {
            _directory: directory,
            root,
            archive,
        }
    }

    fn receipt_path(&self) -> PathBuf {
        self.root.join(IMPORT_RECEIPT)
    }

    fn assert_status(&self, status: ImportStatus) {
        assert!(read_receipt(&self.root, &CheckpointLimits::default())
            .unwrap()
            .is_some_and(|receipt| receipt.status == status));
    }

    fn import(&self) -> TreeImportReceipt {
        import_locked(&self.root, &self.archive).unwrap()
    }

    fn record_import(&self) -> Vec<PublicationStep> {
        let mut steps = vec![];
        import_locked_with(&self.root, &self.archive, |step| {
            step.perform()?;
            steps.push(step.clone());
            Ok(())
        })
        .unwrap();
        steps
    }
}

fn sync_failure() -> Error {
    Error::Io(std::io::Error::other("injected import sync failure"))
}

fn assert_sync_failure(result: Result<TreeImportReceipt>) {
    assert!(
        matches!(result, Err(Error::Io(error)) if error.to_string() == "injected import sync failure")
    );
}

fn directory_index(steps: &[PublicationStep], directory: &Path) -> usize {
    steps
        .iter()
        .position(|step| matches!(step, PublicationStep::SyncDirectory(path) if path == directory))
        .unwrap_or_else(|| panic!("missing directory sync: {}", directory.display()))
}

fn assert_canonical_ancestry_synced_after_root(steps: &[PublicationStep], root: &Path) {
    let mut after = directory_index(steps, root) + 1;
    for ancestor in root.ancestors().skip(1) {
        let offset = steps[after..]
            .iter()
            .position(
                |step| matches!(step, PublicationStep::SyncDirectory(path) if path == ancestor),
            )
            .unwrap_or_else(|| panic!("missing ordered ancestor sync: {}", ancestor.display()));
        after += offset + 1;
    }
}

#[test]
fn pending_receipt_ancestry_failure_refuses_publication_before_any_selected_file() {
    let fixture = ImportFixture::new(&[("nested/file.bin", EntryKind::File, b"payload")]);
    let target = fixture.root.join("nested/file.bin");
    for _ in 0..2 {
        let mut attempted = false;
        assert_sync_failure(import_locked_with(
            &fixture.root,
            &fixture.archive,
            |step| {
                if matches!(step, PublicationStep::SyncDirectory(path) if path == &fixture.root)
                    && !target.exists()
                    && read_receipt(&fixture.root, &CheckpointLimits::default())?
                        .is_some_and(|receipt| receipt.status == ImportStatus::Pending)
                {
                    attempted = true;
                    return Err(sync_failure());
                }
                step.perform()
            },
        ));
        assert!(attempted);
        assert!(!target.exists());
        fixture.assert_status(ImportStatus::Pending);
    }
    fixture.import();
    assert_eq!(fs::read(target).unwrap(), b"payload");
    fixture.assert_status(ImportStatus::Complete);
}

#[test]
fn pending_matching_retry_repeats_failed_ancestor_sync_before_completing() {
    let fixture = ImportFixture::new(&[("nested/deep/file.bin", EntryKind::File, b"payload")]);
    let target = fixture.root.join("nested/deep/file.bin");
    let fail_directory = fixture.root.join("nested");
    for _ in 0..2 {
        let mut attempted = false;
        let result = import_locked_with(&fixture.root, &fixture.archive, |step| {
            if matches!(step, PublicationStep::SyncDirectory(path) if path == &fail_directory) {
                attempted = true;
                return Err(sync_failure());
            }
            step.perform()
        });
        assert_sync_failure(result);
        assert!(attempted);
        assert_eq!(fs::read(&target).unwrap(), b"payload");
        fixture.assert_status(ImportStatus::Pending);
    }
    let steps = fixture.record_import();
    assert!(steps
        .iter()
        .any(|step| matches!(step, PublicationStep::SyncFile(path) if path == &target)));
    let nested_sync = directory_index(&steps, &fail_directory);
    assert!(steps
        .iter()
        .skip(nested_sync + 1)
        .any(|step| matches!(step, PublicationStep::SyncDirectory(path) if path == &fixture.root)));
    fixture.assert_status(ImportStatus::Complete);
}

#[test]
fn complete_retry_selected_file_sync_failure_refuses_ack_without_overwriting() {
    let fixture = ImportFixture::new(&[("file.bin", EntryKind::File, b"payload")]);
    fixture.import();
    let target = fixture.root.join("file.bin");
    let before = fs::metadata(&target).unwrap();
    let receipt = fs::read(fixture.receipt_path()).unwrap();
    let mut attempted = false;
    assert_sync_failure(import_locked_with(
        &fixture.root,
        &fixture.archive,
        |step| {
            if matches!(step, PublicationStep::SyncFile(path) if path == &target) {
                attempted = true;
                return Err(sync_failure());
            }
            step.perform()
        },
    ));
    assert!(attempted);
    assert_eq!(fs::read(&target).unwrap(), b"payload");
    assert_eq!(fs::read(fixture.receipt_path()).unwrap(), receipt);
    #[cfg(unix)]
    {
        assert_eq!(fs::metadata(&target).unwrap().ino(), before.ino());
    }
    #[cfg(not(unix))]
    let _ = before;
    fixture.assert_status(ImportStatus::Complete);
    fixture.import();
}

#[test]
fn complete_retry_receipt_sync_failure_refuses_ack_and_retries() {
    let fixture = ImportFixture::new(&[("file.bin", EntryKind::File, b"payload")]);
    fixture.import();
    let receipt_path = fixture.receipt_path();
    let receipt = fs::read(&receipt_path).unwrap();
    for _ in 0..2 {
        let mut attempted = false;
        assert_sync_failure(import_locked_with(
            &fixture.root,
            &fixture.archive,
            |step| {
                if matches!(step, PublicationStep::SyncFile(path) if path == &receipt_path) {
                    attempted = true;
                    return Err(sync_failure());
                }
                step.perform()
            },
        ));
        assert!(attempted);
        assert_eq!(fs::read(&receipt_path).unwrap(), receipt);
        fixture.assert_status(ImportStatus::Complete);
    }
    assert_eq!(fixture.import().files, 1);
}

#[test]
fn complete_receipt_visible_before_failed_directory_sync_is_not_acknowledged() {
    let fixture = ImportFixture::new(&[("file.bin", EntryKind::File, b"payload")]);
    let control = fixture.root.join(".workspace-sdk");
    for _ in 0..2 {
        let mut attempted = false;
        let result = import_locked_with(&fixture.root, &fixture.archive, |step| {
            if matches!(step, PublicationStep::SyncDirectory(path) if path == &control)
                && read_receipt(&fixture.root, &CheckpointLimits::default())?
                    .is_some_and(|receipt| receipt.status == ImportStatus::Complete)
            {
                attempted = true;
                // This is the actual receipt-directory fsync boundary, before
                // the callback performs it, after the atomic persist is visible.
                return Err(sync_failure());
            }
            step.perform()
        });
        assert_sync_failure(result);
        assert!(attempted);
        fixture.assert_status(ImportStatus::Complete);
        assert_eq!(fs::read(fixture.root.join("file.bin")).unwrap(), b"payload");
    }
    assert_eq!(fixture.import().files, 1);
}

#[test]
fn complete_retry_flushes_nested_directories_bottom_up_and_receipt_ancestry() {
    let fixture = ImportFixture::new(&[("nested/deep/file.bin", EntryKind::File, b"payload")]);
    fixture.import();
    let steps = fixture.record_import();
    let file_index = steps.iter().position(|step| matches!(step, PublicationStep::SyncFile(path) if path == &fixture.root.join("nested/deep/file.bin"))).unwrap();
    let deep = directory_index(&steps, &fixture.root.join("nested/deep"));
    let nested = directory_index(&steps, &fixture.root.join("nested"));
    let root = directory_index(&steps, &fixture.root);
    assert!(file_index < deep && deep < nested && nested < root);
    let receipt_index = steps.iter().position(|step| matches!(step, PublicationStep::SyncFile(path) if path == &fixture.receipt_path())).unwrap();
    let control = directory_index(&steps, &fixture.root.join(".workspace-sdk"));
    assert!(receipt_index < control);
    assert!(steps
        .iter()
        .skip(control + 1)
        .any(|step| matches!(step, PublicationStep::SyncDirectory(path) if path == &fixture.root)));
    assert!(steps.iter().all(|step| matches!(
        step,
        PublicationStep::SyncFile(_) | PublicationStep::SyncDirectory(_)
    )));
    assert_canonical_ancestry_synced_after_root(&steps, &fixture.root);
}

#[test]
fn empty_tree_retry_still_flushes_project_root_and_receipt() {
    let fixture = ImportFixture::new(&[]);
    fixture.import();
    let steps = fixture.record_import();
    directory_index(&steps, &fixture.root);
    assert!(steps.iter().any(
        |step| matches!(step, PublicationStep::SyncFile(path) if path == &fixture.receipt_path())
    ));
    assert_canonical_ancestry_synced_after_root(&steps, &fixture.root);
}

#[cfg(unix)]
#[test]
fn symlink_retry_syncs_parent_without_opening_link_or_unselected_target() {
    let fixture = ImportFixture::new(&[("nested/link", EntryKind::Symlink, b"../absent")]);
    fixture.import();
    let steps = fixture.record_import();
    assert_eq!(
        fs::read_link(fixture.root.join("nested/link")).unwrap(),
        Path::new("../absent")
    );
    assert!(!fixture.root.join("absent").exists());
    assert!(steps.iter().all(|step| !matches!(step, PublicationStep::SyncFile(path) if path == &fixture.root.join("nested/link") || path == &fixture.root.join("absent"))));
    assert!(
        directory_index(&steps, &fixture.root.join("nested"))
            < directory_index(&steps, &fixture.root)
    );
}

#[test]
fn changed_pending_file_conflicts_before_retry_sync_or_overwrite() {
    let fixture = ImportFixture::new(&[("nested/file.bin", EntryKind::File, b"payload")]);
    let target = fixture.root.join("nested/file.bin");
    let mut injected = false;
    assert_sync_failure(import_locked_with(
        &fixture.root,
        &fixture.archive,
        |step| {
            if matches!(step, PublicationStep::SyncDirectory(path) if path == &fixture.root)
                && fs::read(&target).is_ok_and(|bytes| bytes == b"payload")
                && read_receipt(&fixture.root, &CheckpointLimits::default())?
                    .is_some_and(|receipt| receipt.status == ImportStatus::Pending)
            {
                injected = true;
                return Err(sync_failure());
            }
            step.perform()
        },
    ));
    assert!(injected);
    fixture.assert_status(ImportStatus::Pending);
    fs::write(&target, b"user edit").unwrap();
    let mut calls = 0;
    let result = import_locked_with(&fixture.root, &fixture.archive, |step| {
        calls += 1;
        step.perform()
    });
    assert!(matches!(result, Err(Error::Conflict(_))));
    assert_eq!(calls, 0);
    assert_eq!(fs::read(target).unwrap(), b"user edit");
    fixture.assert_status(ImportStatus::Pending);
}
