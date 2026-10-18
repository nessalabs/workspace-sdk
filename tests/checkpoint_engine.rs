use std::fs;
use std::path::Path;
use std::process::Command;
use workspace_sdk::checkpoint::*;

fn engine(root: &Path) -> GitCheckpointEngine {
    GitCheckpointEngine::new("git", root.join("state")).unwrap()
}

fn directory(root: &Path, name: &str) -> std::path::PathBuf {
    let path = root.join(name);
    fs::create_dir_all(&path).unwrap();
    path
}

fn write(root: &Path, name: &str, bytes: impl AsRef<[u8]>) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[test]
fn portable_non_git_snapshot_preserves_binary_modes_symlinks_and_filters() {
    let temp = tempfile::tempdir().unwrap();
    let source = directory(temp.path(), "source");
    let sdk = engine(temp.path());
    write(&source, "binary.bin", [0, 255, 1, 2, 0, 128]);
    write(&source, "script.sh", "#!/bin/sh\necho test\n");
    write(&source, ".env", "API_KEY=private");
    write(&source, "secrets/server.pem", "private key");
    write(&source, "node_modules/package/index.js", "dependency");
    write(&source, ".workspace-sdk/run.json", "private state");
    write(&source, ".gitignore", "ignored.txt\n");
    write(&source, "ignored.txt", "ignored");
    write(&source, ".workspaceignore", "excluded.txt\n");
    write(&source, "excluded.txt", "ignored");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(source.join("script.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("binary.bin", source.join("link")).unwrap();
    }
    let checkpoint = sdk.capture(&source, CaptureOptions::default()).unwrap();
    let names = checkpoint
        .manifest
        .entries
        .iter()
        .map(|e| e.path.as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"binary.bin"));
    assert!(!names.contains(&".env"));
    assert!(!names.contains(&"ignored.txt"));
    assert!(!names.contains(&"excluded.txt"));
    let target_engine = GitCheckpointEngine::new("git", temp.path().join("target-state")).unwrap();
    let imported = target_engine
        .import(&sdk.export(checkpoint.reference()).unwrap())
        .unwrap();
    let restored = temp.path().join("restored");
    target_engine
        .restore(imported.reference(), &restored)
        .unwrap();
    assert_eq!(
        fs::read(restored.join("binary.bin")).unwrap(),
        [0, 255, 1, 2, 0, 128]
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            fs::metadata(restored.join("script.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0
        );
        assert_eq!(
            fs::read_link(restored.join("link")).unwrap(),
            Path::new("binary.bin")
        );
    }
    assert!(!restored.join(".git").exists());
    assert!(!restored.join(".env").exists());
}

#[test]
fn tracked_secrets_are_removed_and_git_index_is_untouched() {
    let temp = tempfile::tempdir().unwrap();
    let source = directory(temp.path(), "source");
    let sdk = engine(temp.path());
    git(&source, &["init", "--quiet"]);
    write(&source, "keep.txt", "tracked despite ignore");
    write(&source, ".env.production", "credential");
    write(&source, "omit.txt", "workspace filtered");
    git(&source, &["add", "."]);
    write(&source, ".gitignore", "keep.txt\n");
    write(&source, ".workspaceignore", "omit.txt\n");
    let original = fs::read(source.join(".git/index")).unwrap();
    let checkpoint = sdk.capture(&source, CaptureOptions::default()).unwrap();
    let names = checkpoint
        .manifest
        .entries
        .iter()
        .map(|e| e.path.as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"keep.txt"));
    assert!(!names.contains(&"omit.txt"));
    assert!(!names.contains(&".env.production"));
    assert_eq!(original, fs::read(source.join(".git/index")).unwrap());
}

// Only disposable fixture repositories use this configuration isolation. The
// checkpoint engine still exercises its real source-repository inspection.
fn linked_fixture_git(root: &Path, args: &[&str]) -> Vec<u8> {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_str().is_some_and(|key| key.starts_with("GIT_")) {
            command.env_remove(key);
        }
    }
    let output = command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgSign=false",
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-C",
        ])
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "fixture Git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

struct LinkedCheckpointFixture {
    temporary: tempfile::TempDir,
    main: std::path::PathBuf,
    linked: std::path::PathBuf,
    pointer: Vec<u8>,
    metadata: std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>,
}

fn metadata_bytes(root: &Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    let mut files = std::collections::BTreeMap::new();
    let mut directories = vec![root.to_owned()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            let kind = fs::symlink_metadata(&path).unwrap().file_type();
            if kind.is_dir() {
                directories.push(path);
            } else {
                assert!(kind.is_file(), "fixture metadata must be regular");
                files.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    files
}

impl LinkedCheckpointFixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let main = directory(temporary.path(), "main").canonicalize().unwrap();
        linked_fixture_git(
            &main,
            &["init", "--quiet", "--template=", "--initial-branch=main"],
        );
        linked_fixture_git(&main, &["config", "--local", "core.hooksPath", "/dev/null"]);
        linked_fixture_git(&main, &["config", "--local", "commit.gpgSign", "false"]);
        write(&main, ".gitignore", "tracked.txt\nignored.txt\n");
        write(
            &main,
            "tracked.txt",
            "tracked bytes must survive selection\n",
        );
        write(&main, "nested/tracked.txt", "nested tracked bytes\n");
        linked_fixture_git(&main, &["add", ".gitignore"]);
        linked_fixture_git(
            &main,
            &["add", "--force", "tracked.txt", "nested/tracked.txt"],
        );
        linked_fixture_git(
            &main,
            &["commit", "--quiet", "-m", "tracked ignored fixture"],
        );
        let linked = temporary.path().canonicalize().unwrap().join("linked");
        linked_fixture_git(
            &main,
            &[
                "worktree",
                "add",
                "--quiet",
                "--no-track",
                "-b",
                "linked",
                linked.to_str().unwrap(),
                "HEAD",
            ],
        );
        assert!(
            linked.join(".git").is_file(),
            "must exercise a linked .git pointer"
        );
        assert_eq!(
            linked_fixture_git(&linked, &["ls-files", "--error-unmatch", "tracked.txt"]),
            b"tracked.txt\n"
        );
        assert_eq!(
            linked_fixture_git(&linked, &["check-ignore", "--no-index", "tracked.txt"]),
            b"tracked.txt\n"
        );
        write(&linked, "ignored.txt", "untracked ignored data\n");
        let pointer = fs::read(linked.join(".git")).unwrap();
        let metadata = metadata_bytes(&main.join(".git"));
        Self {
            temporary,
            main,
            linked,
            pointer,
            metadata,
        }
    }

    fn assert_source_unchanged(&self, metadata_root: &Path) {
        assert_eq!(fs::read(self.linked.join(".git")).unwrap(), self.pointer);
        assert_eq!(metadata_bytes(metadata_root), self.metadata);
        assert_eq!(
            fs::read(self.linked.join("tracked.txt")).unwrap(),
            b"tracked bytes must survive selection\n"
        );
        assert_eq!(
            fs::read(self.linked.join("ignored.txt")).unwrap(),
            b"untracked ignored data\n"
        );
        assert_eq!(
            fs::read(self.linked.join("nested/tracked.txt")).unwrap(),
            b"nested tracked bytes\n"
        );
    }

    fn make_common_metadata_unavailable(&self) -> std::path::PathBuf {
        // Rename only this fixture's exclusively owned common metadata. The
        // linked pointer remains unchanged and now names unavailable metadata.
        let retained = self.temporary.path().join("retained-git-metadata");
        fs::rename(self.main.join(".git"), &retained).unwrap();
        assert!(!self.main.join(".git").exists());
        assert_eq!(metadata_bytes(&retained), self.metadata);
        retained
    }
}

#[test]
fn linked_worktree_capture_and_tree_export_preserve_tracked_ignore_selection_and_source_metadata() {
    let fixture = LinkedCheckpointFixture::new();
    let sdk = engine(fixture.temporary.path());
    let checkpoint = sdk
        .capture(&fixture.linked, CaptureOptions::default())
        .unwrap();
    let paths: Vec<_> = checkpoint
        .manifest
        .entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    assert!(paths.contains(&"tracked.txt"));
    assert!(paths.contains(&"nested/tracked.txt"));
    assert!(!paths.contains(&"ignored.txt"));
    assert!(!paths
        .iter()
        .any(|path| *path == ".git" || path.starts_with(".git/")));
    let archive = sdk.export_tree_archive(&fixture.linked).unwrap();
    let restored = fixture.temporary.path().join("restored");
    sdk.import_tree_archive(&archive, &restored).unwrap();
    assert_eq!(
        fs::read(restored.join("tracked.txt")).unwrap(),
        b"tracked bytes must survive selection\n"
    );
    assert_eq!(
        fs::read(restored.join("nested/tracked.txt")).unwrap(),
        b"nested tracked bytes\n"
    );
    assert!(!restored.join("ignored.txt").exists());
    assert!(!restored.join(".git").exists());
    fixture.assert_source_unchanged(&fixture.main.join(".git"));
}

#[test]
fn linked_worktree_capture_refuses_unavailable_common_metadata_instead_of_omitting_tracked_files() {
    let fixture = LinkedCheckpointFixture::new();
    let sdk = engine(fixture.temporary.path());
    let retained = fixture.make_common_metadata_unavailable();
    assert!(
        sdk.capture(&fixture.linked, CaptureOptions::default())
            .is_err(),
        "a broken Git pointer must not downgrade tracked files to ignored untracked files"
    );
    fixture.assert_source_unchanged(&retained);
}

#[test]
fn linked_worktree_tree_export_refuses_unavailable_common_metadata_instead_of_omitting_tracked_files(
) {
    let fixture = LinkedCheckpointFixture::new();
    let sdk = engine(fixture.temporary.path());
    let retained = fixture.make_common_metadata_unavailable();
    assert!(
        sdk.export_tree_archive(&fixture.linked).is_err(),
        "tree export must not acknowledge successful omission after failed Git discovery"
    );
    fixture.assert_source_unchanged(&retained);
}

#[test]
fn linked_worktree_subdirectory_refuses_a_broken_ancestor_git_pointer_for_capture_and_export() {
    let fixture = LinkedCheckpointFixture::new();
    let sdk = engine(fixture.temporary.path());
    let nested = fixture.linked.join("nested");
    // Establish that the requested subdirectory belongs to the linked Git index,
    // while its .git marker is on an ancestor rather than the capture root.
    assert!(!nested.join(".git").exists());
    assert_eq!(
        linked_fixture_git(&nested, &["ls-files", "--error-unmatch", "tracked.txt"]),
        b"tracked.txt\n"
    );
    let retained = fixture.make_common_metadata_unavailable();
    let capture = sdk.capture(&nested, CaptureOptions::default());
    let export = sdk.export_tree_archive(&nested);
    assert!(
        capture.is_err(),
        "a broken ancestor .git must not be treated as an ordinary non-Git directory"
    );
    assert!(
        export.is_err(),
        "export must also refuse failed ancestor Git discovery"
    );
    fixture.assert_source_unchanged(&retained);
}

#[test]
fn clean_merge_stages_then_explicit_apply_and_rollback_preserve_excluded_files() {
    let temp = tempfile::tempdir().unwrap();
    let local = directory(temp.path(), "local");
    let remote = directory(temp.path(), "remote");
    let sdk = engine(temp.path());
    write(&local, "local.txt", "base local\n");
    write(&local, "remote.txt", "base remote\n");
    write(&local, "deleted.txt", "delete\n");
    write(&local, ".env", "local secret");
    let base = sdk.capture(&local, CaptureOptions::default()).unwrap();
    sdk.restore(base.reference(), &remote).unwrap();
    write(&local, "local.txt", "local edit\n");
    write(&remote, "remote.txt", "remote edit\n");
    fs::remove_file(remote.join("deleted.txt")).unwrap();
    let result = sdk.capture(&remote, CaptureOptions::default()).unwrap();
    let staged = sdk
        .reconcile(base.reference(), &local, result.reference())
        .unwrap();
    assert_eq!(staged.status, ReconciliationStatus::StagedForReview);
    assert_eq!(
        fs::read_to_string(local.join("remote.txt")).unwrap(),
        "base remote\n"
    );
    assert!(sdk
        .apply(
            &staged.reference,
            ApplyOptions {
                quiescent: false,
                expected_fingerprint: staged.expected_fingerprint.clone()
            }
        )
        .is_err());
    let receipt = sdk
        .apply(
            &staged.reference,
            ApplyOptions {
                quiescent: true,
                expected_fingerprint: staged.expected_fingerprint,
            },
        )
        .unwrap();
    assert_eq!(
        fs::read_to_string(local.join("local.txt")).unwrap(),
        "local edit\n"
    );
    assert_eq!(
        fs::read_to_string(local.join("remote.txt")).unwrap(),
        "remote edit\n"
    );
    assert!(!local.join("deleted.txt").exists());
    assert_eq!(
        fs::read_to_string(local.join(".env")).unwrap(),
        "local secret"
    );
    sdk.rollback(&receipt.recovery, true).unwrap();
    assert_eq!(
        fs::read_to_string(local.join("remote.txt")).unwrap(),
        "base remote\n"
    );
    assert!(local.join("deleted.txt").exists());
    assert_eq!(
        fs::read_to_string(local.join(".env")).unwrap(),
        "local secret"
    );
}

#[test]
fn conflicting_edits_preserve_all_originals_and_require_explicit_resolution() {
    let temp = tempfile::tempdir().unwrap();
    let local = directory(temp.path(), "local");
    let remote = directory(temp.path(), "remote");
    let sdk = engine(temp.path());
    write(&local, "file.txt", "base\n");
    let base = sdk.capture(&local, CaptureOptions::default()).unwrap();
    sdk.restore(base.reference(), &remote).unwrap();
    write(&local, "file.txt", "local\n");
    write(&remote, "file.txt", "remote\n");
    let remote_checkpoint = sdk.capture(&remote, CaptureOptions::default()).unwrap();
    let staged = sdk
        .reconcile(base.reference(), &local, remote_checkpoint.reference())
        .unwrap();
    assert_eq!(staged.status, ReconciliationStatus::Conflicted);
    assert_eq!(staged.conflicts.len(), 1);
    let conflict = &staged.conflicts[0];
    assert_eq!(
        fs::read_to_string(&conflict.base.as_ref().unwrap().preserved_path).unwrap(),
        "base\n"
    );
    assert_eq!(
        fs::read_to_string(&conflict.local.as_ref().unwrap().preserved_path).unwrap(),
        "local\n"
    );
    assert_eq!(
        fs::read_to_string(&conflict.remote.as_ref().unwrap().preserved_path).unwrap(),
        "remote\n"
    );
    assert_eq!(
        fs::read_to_string(local.join("file.txt")).unwrap(),
        "local\n"
    );
    assert!(sdk.accept_resolution(&staged.reference, &[]).is_err());
    write(&staged.review_directory, "file.txt", "chosen resolution\n");
    let resolved = sdk
        .accept_resolution(&staged.reference, &["file.txt".into()])
        .unwrap();
    sdk.apply(
        &resolved.reference,
        ApplyOptions {
            quiescent: true,
            expected_fingerprint: resolved.expected_fingerprint,
        },
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(local.join("file.txt")).unwrap(),
        "chosen resolution\n"
    );
}

#[test]
fn stale_destination_and_changed_review_cannot_be_applied() {
    let temp = tempfile::tempdir().unwrap();
    let local = directory(temp.path(), "local");
    let remote = directory(temp.path(), "remote");
    let sdk = engine(temp.path());
    write(&local, "file.txt", "base\n");
    let base = sdk.capture(&local, CaptureOptions::default()).unwrap();
    sdk.restore(base.reference(), &remote).unwrap();
    write(&remote, "file.txt", "remote\n");
    let remote_checkpoint = sdk.capture(&remote, CaptureOptions::default()).unwrap();
    let staged = sdk
        .reconcile(base.reference(), &local, remote_checkpoint.reference())
        .unwrap();
    write(&local, "file.txt", "late editor\n");
    assert!(sdk
        .apply(
            &staged.reference,
            ApplyOptions {
                quiescent: true,
                expected_fingerprint: staged.expected_fingerprint.clone()
            }
        )
        .unwrap_err()
        .to_string()
        .contains("destination changed"));
    assert_eq!(
        fs::read_to_string(local.join("file.txt")).unwrap(),
        "late editor\n"
    );
    write(&local, "file.txt", "base\n");
    write(&staged.review_directory, "file.txt", "review edit\n");
    assert!(sdk
        .apply(
            &staged.reference,
            ApplyOptions {
                quiescent: true,
                expected_fingerprint: staged.expected_fingerprint
            }
        )
        .unwrap_err()
        .to_string()
        .contains("review directory changed"));
}

#[test]
fn tree_archive_validates_paths_symlinks_hashes_and_limits_before_writing() {
    let temp = tempfile::tempdir().unwrap();
    let source = directory(temp.path(), "source");
    let sdk = engine(temp.path());
    write(&source, "good.txt", "good");
    let bytes = sdk.export_tree_archive(&source).unwrap();
    let archive: TreeArchive = serde_json::from_slice(&bytes).unwrap();
    for path in [
        "../escape",
        "/absolute",
        ".git/config",
        "nested/.env",
        "C:\\escape",
        "a//b",
    ] {
        let mut bad = archive.clone();
        bad.entries[0].file.path = path.into();
        let dest = temp.path().join("destination");
        assert!(
            sdk.import_tree_archive(&serde_json::to_vec(&bad).unwrap(), &dest)
                .is_err(),
            "{path}"
        );
        assert!(!dest.exists());
    }
    let mut bad = archive.clone();
    bad.entries[0].data_base64 = "ZXZpbA==".into();
    assert!(sdk
        .import_tree_archive(
            &serde_json::to_vec(&bad).unwrap(),
            &temp.path().join("bad-checksum")
        )
        .is_err());
    let mut overlap = archive.clone();
    overlap.entries.push(overlap.entries[0].clone());
    overlap.entries[1].file.path = "good.txt/child".into();
    assert!(sdk
        .import_tree_archive(
            &serde_json::to_vec(&overlap).unwrap(),
            &temp.path().join("overlap")
        )
        .is_err());
    let tiny = GitCheckpointEngine::new("git", temp.path().join("tiny-state"))
        .unwrap()
        .with_limits(CheckpointLimits {
            max_total_bytes: 1,
            ..Default::default()
        })
        .unwrap();
    assert!(tiny
        .import_tree_archive(&bytes, &temp.path().join("tiny"))
        .is_err());
    sdk.import_tree_archive(&bytes, &temp.path().join("valid"))
        .unwrap();
    assert_eq!(
        fs::read_to_string(temp.path().join("valid/good.txt")).unwrap(),
        "good"
    );
}

#[test]
fn bundle_tampering_and_reference_traversal_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let source = directory(temp.path(), "source");
    let sdk = engine(temp.path());
    write(&source, "file", "content");
    let snapshot = sdk.capture(&source, CaptureOptions::default()).unwrap();
    let mut bundle = sdk.export(snapshot.reference()).unwrap();
    bundle.bytes[0] ^= 1;
    assert!(sdk.import(&bundle).is_err());
    let mut bundle = sdk.export(snapshot.reference()).unwrap();
    bundle.manifest.reference = CheckpointRef("../escape".into());
    assert!(sdk.import(&bundle).is_err());
    assert!(sdk.export(&CheckpointRef("../../source".into())).is_err());
    assert!(sdk.restore(snapshot.reference(), &source).is_err());
}

#[test]
fn source_history_and_attributes_do_not_enter_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let source = directory(temp.path(), "source");
    let sdk = engine(temp.path());
    git(&source, &["init", "--quiet"]);
    write(&source, "file", "historic secret");
    git(&source, &["add", "."]);
    git(
        &source,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-qm",
            "history",
        ],
    );
    write(&source, "file", b"now\r\n");
    write(&source, ".gitattributes", "file text eol=lf\n");
    let checkpoint = sdk.capture(&source, CaptureOptions::default()).unwrap();
    let destination = temp.path().join("restored");
    sdk.restore(checkpoint.reference(), &destination).unwrap();
    assert_eq!(fs::read(destination.join("file")).unwrap(), b"now\r\n");
    let stored = sdk
        .state_directory()
        .join("checkpoints")
        .join(&checkpoint.reference().0)
        .join("repo");
    assert_eq!(
        git(&stored, &["rev-list", "--count", "refs/heads/snapshot"]),
        b"1\n"
    );
}

#[cfg(unix)]
#[test]
fn external_symlinks_and_unresolved_lfs_fail_explicitly() {
    let temp = tempfile::tempdir().unwrap();
    let source = directory(temp.path(), "source");
    let sdk = engine(temp.path());
    std::os::unix::fs::symlink("../../outside", source.join("unsafe")).unwrap();
    assert!(sdk.capture(&source, CaptureOptions::default()).is_err());
    fs::remove_file(source.join("unsafe")).unwrap();
    write(
        &source,
        "large.bin",
        "version https://git-lfs.github.com/spec/v1\noid sha256:abc\nsize 5000\n",
    );
    assert!(sdk
        .capture(&source, CaptureOptions::default())
        .unwrap_err()
        .to_string()
        .contains("LFS"));
}

#[test]
fn snapshots_export_and_capture_archives_without_losing_tracked_ignore_selection() {
    let temp = tempfile::tempdir().unwrap();
    let source = directory(temp.path(), "source");
    let sdk = engine(temp.path());
    git(&source, &["init", "--quiet"]);
    write(&source, "tracked.txt", "tracked");
    git(&source, &["add", "."]);
    write(&source, ".gitignore", "tracked.txt\n");
    let original = sdk.capture(&source, CaptureOptions::default()).unwrap();
    let archive = sdk.export_checkpoint_archive(original.reference()).unwrap();
    let restored = sdk
        .capture_archive(&archive, CaptureOptions::default())
        .unwrap();
    assert_eq!(
        original.manifest.tree_fingerprint,
        restored.manifest.tree_fingerprint
    );
}

#[test]
fn apply_file_directory_transition_and_failed_apply_rollback_are_recoverable() {
    let temp = tempfile::tempdir().unwrap();
    let local = directory(temp.path(), "local");
    let remote = directory(temp.path(), "remote");
    let sdk = engine(temp.path());
    write(&local, "directory/file.txt", "base\n");
    write(&local, "other.txt", "unchanged\n");
    let base = sdk.capture(&local, CaptureOptions::default()).unwrap();
    sdk.restore(base.reference(), &remote).unwrap();
    fs::remove_dir_all(remote.join("directory")).unwrap();
    write(&remote, "directory", "replacement file\n");
    let changed = sdk.capture(&remote, CaptureOptions::default()).unwrap();
    let staged = sdk
        .reconcile(base.reference(), &local, changed.reference())
        .unwrap();
    let receipt = sdk
        .apply(
            &staged.reference,
            ApplyOptions {
                quiescent: true,
                expected_fingerprint: staged.expected_fingerprint,
            },
        )
        .unwrap();
    assert_eq!(
        fs::read_to_string(local.join("directory")).unwrap(),
        "replacement file\n"
    );
    sdk.rollback(&receipt.recovery, true).unwrap();
    assert_eq!(
        fs::read_to_string(local.join("directory/file.txt")).unwrap(),
        "base\n"
    );
    // This excluded local file prevents replacing its parent directory. The SDK
    // must preserve it and restore selected originals after the failed apply.
    write(&local, "directory/.env", "secret");
    let staged = sdk
        .reconcile(base.reference(), &local, changed.reference())
        .unwrap();
    let failure = sdk
        .apply(
            &staged.reference,
            ApplyOptions {
                quiescent: true,
                expected_fingerprint: staged.expected_fingerprint,
            },
        )
        .unwrap_err()
        .to_string();
    assert!(failure.contains("rolled back"), "{failure}");
    assert_eq!(
        fs::read_to_string(local.join("directory/.env")).unwrap(),
        "secret"
    );
    assert_eq!(
        fs::read_to_string(local.join("directory/file.txt")).unwrap(),
        "base\n"
    );
    assert_eq!(
        fs::read_to_string(local.join("other.txt")).unwrap(),
        "unchanged\n"
    );
}

#[cfg(unix)]
#[test]
fn chained_symlinks_cannot_escape_through_parent_segments() {
    let temp = tempfile::tempdir().unwrap();
    let root = directory(temp.path(), "source");
    let sdk = engine(temp.path());
    write(&root, "dir/file", "content");
    std::os::unix::fs::symlink("dir", root.join("alias")).unwrap();
    std::os::unix::fs::symlink("..", root.join("dir/back")).unwrap();
    std::os::unix::fs::symlink("alias/back/../outside", root.join("escape")).unwrap();
    assert!(sdk.capture(&root, CaptureOptions::default()).is_err());
}

#[test]
fn apply_recovers_receipt_interruption_and_retries_without_overwriting_later_edits() {
    let temporary = tempfile::tempdir().unwrap();
    let local = directory(temporary.path(), "local");
    let remote = directory(temporary.path(), "remote");
    let sdk = engine(temporary.path());
    write(&local, "file.txt", "base\n");
    let base = sdk.capture(&local, CaptureOptions::default()).unwrap();
    sdk.restore(base.reference(), &remote).unwrap();
    write(&remote, "file.txt", "remote\n");
    let remote = sdk.capture(&remote, CaptureOptions::default()).unwrap();
    let review = sdk
        .reconcile(base.reference(), &local, remote.reference())
        .unwrap();
    let options = ApplyOptions {
        quiescent: true,
        expected_fingerprint: review.expected_fingerprint.clone(),
    };
    assert!(matches!(
        sdk.inspect_apply(&review.reference).unwrap(),
        ApplyState::NotStarted
    ));
    let first = sdk.apply(&review.reference, options.clone()).unwrap();
    // Model process death after tree publication but before either completion
    // receipt. The persisted apply intent still identifies the original backup.
    let reconciliation_path = sdk
        .state_directory()
        .join("reconciliations")
        .join(&review.reference.0)
        .join("reconciliation.json");
    let mut reconciliation: serde_json::Value =
        serde_json::from_slice(&fs::read(&reconciliation_path).unwrap()).unwrap();
    reconciliation["status"] = "staged_for_review".into();
    reconciliation["applied"] = serde_json::Value::Null;
    fs::write(
        &reconciliation_path,
        serde_json::to_vec(&reconciliation).unwrap(),
    )
    .unwrap();
    let journal_path = sdk
        .state_directory()
        .join("recoveries")
        .join(&first.recovery.0)
        .join("journal.json");
    let mut journal: serde_json::Value =
        serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
    journal["status"] = "applying".into();
    fs::write(&journal_path, serde_json::to_vec(&journal).unwrap()).unwrap();
    let reconciliation_before = fs::read(&reconciliation_path).unwrap();
    let journal_before = fs::read(&journal_path).unwrap();
    assert!(
        matches!(sdk.inspect_apply(&review.reference).unwrap(), ApplyState::Interrupted { recovery } if recovery == first.recovery)
    );
    assert_eq!(
        reconciliation_before,
        fs::read(&reconciliation_path).unwrap()
    );
    assert_eq!(journal_before, fs::read(&journal_path).unwrap());
    let recovered = sdk.apply(&review.reference, options.clone()).unwrap();
    assert_eq!(first.recovery, recovered.recovery);
    assert_eq!(first.checkpoint, recovered.checkpoint);
    assert!(
        matches!(sdk.inspect_apply(&review.reference).unwrap(), ApplyState::Applied { receipt } if receipt.recovery == first.recovery)
    );
    write(&local, "file.txt", "later editor change\n");
    let retried = sdk.apply(&review.reference, options).unwrap();
    assert_eq!(retried.recovery, first.recovery);
    assert_eq!(
        fs::read_to_string(local.join("file.txt")).unwrap(),
        "later editor change\n"
    );
    assert!(sdk
        .apply(
            &review.reference,
            ApplyOptions {
                quiescent: true,
                expected_fingerprint: "wrong".into()
            }
        )
        .is_err());
    assert!(sdk
        .apply(
            &review.reference,
            ApplyOptions {
                quiescent: false,
                expected_fingerprint: review.expected_fingerprint
            }
        )
        .is_err());
    sdk.rollback(&first.recovery, true).unwrap();
    assert!(
        matches!(sdk.inspect_apply(&review.reference).unwrap(), ApplyState::RolledBack { recovery } if recovery == first.recovery)
    );
}

#[test]
fn linked_worktree_target_export_refuses_broken_git_before_archive_publication() {
    let fixture = LinkedCheckpointFixture::new();
    let transfer = workspace_sdk::TreeTransfer {
        id: workspace_sdk::OperationId::new("broken-linked-export").unwrap(),
    };
    let archive = fixture
        .linked
        .join(transfer.relative_archive_path().as_str());
    let retained = fixture.make_common_metadata_unavailable();
    assert!(export_remote_tree(&fixture.linked, &transfer).is_err());
    assert!(
        !archive.exists(),
        "failed discovery must not publish an archive"
    );
    fixture.assert_source_unchanged(&retained);
}

// Exercise the existing native selected-tree receipt, rather than recapturing a
// plain directory that no longer has Git's tracked-but-ignored selection.
fn linked_return_remote(
    sdk: &GitCheckpointEngine,
    fixture: &LinkedCheckpointFixture,
    base: &CheckpointRef,
) -> std::path::PathBuf {
    let remote = directory(fixture.temporary.path(), "return-remote");
    let transfer = workspace_sdk::TreeTransfer {
        id: workspace_sdk::OperationId::new("linked-outbound").unwrap(),
    };
    let archive = remote.join(transfer.relative_archive_path().as_str());
    fs::create_dir_all(archive.parent().unwrap()).unwrap();
    fs::write(&archive, sdk.export_checkpoint_archive(base).unwrap()).unwrap();
    import_remote_tree(&remote, &transfer).unwrap();
    assert!(!remote.join(".git").exists());
    assert!(!remote.join(".env").exists());
    assert!(!remote.join("ignored.txt").exists());
    assert_eq!(
        fs::read(remote.join("tracked.txt")).unwrap(),
        b"unstaged working bytes\n"
    );
    remote
}

fn capture_linked_return(sdk: &GitCheckpointEngine, remote: &Path) -> Checkpoint {
    let transfer = workspace_sdk::TreeTransfer {
        id: workspace_sdk::OperationId::new("linked-return").unwrap(),
    };
    export_remote_tree(remote, &transfer).unwrap();
    sdk.capture_archive(
        &fs::read(remote.join(transfer.relative_archive_path().as_str())).unwrap(),
        CaptureOptions::default(),
    )
    .unwrap()
}

fn prepare_linked_return(fixture: &LinkedCheckpointFixture) {
    write(&fixture.linked, "tracked.txt", "staged index bytes\n");
    linked_fixture_git(&fixture.linked, &["add", "--force", "tracked.txt"]);
    write(&fixture.linked, "tracked.txt", "unstaged working bytes\n");
    write(
        &fixture.linked,
        "selected-untracked.txt",
        "selected untracked bytes\n",
    );
    write(&fixture.linked, ".env", "excluded local data\n");
}

fn assert_linked_return_metadata(
    fixture: &LinkedCheckpointFixture,
    main_before: &std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>,
) {
    assert_eq!(
        fs::read(fixture.linked.join(".git")).unwrap(),
        fixture.pointer
    );
    // Includes the main checkout plus every common-repository file, including
    // both worktree indexes, refs, logs and objects. No Git command may refresh
    // these files after the intentional staging baseline.
    assert_eq!(metadata_bytes(&fixture.main), *main_before);
    assert_eq!(
        fs::read(fixture.linked.join(".env")).unwrap(),
        b"excluded local data\n"
    );
    assert_eq!(
        fs::read(fixture.linked.join("ignored.txt")).unwrap(),
        b"untracked ignored data\n"
    );
}

#[test]
fn linked_worktree_return_applies_selected_files_and_retries_without_changing_git_metadata() {
    let fixture = LinkedCheckpointFixture::new();
    prepare_linked_return(&fixture);
    let main_before = metadata_bytes(&fixture.main);
    let sdk = engine(fixture.temporary.path());
    let base = sdk
        .capture(&fixture.linked, CaptureOptions::default())
        .unwrap();
    assert_linked_return_metadata(&fixture, &main_before);
    let remote = linked_return_remote(&sdk, &fixture, base.reference());
    assert_linked_return_metadata(&fixture, &main_before);
    write(&remote, "tracked.txt", "remote returned working bytes\n");
    fs::remove_file(remote.join("nested/tracked.txt")).unwrap();
    write(&remote, "remote-new.txt", "remote created bytes\n");
    let returned = capture_linked_return(&sdk, &remote);
    assert!(returned
        .manifest
        .entries
        .iter()
        .any(|entry| entry.path == "tracked.txt"));
    assert!(!returned
        .manifest
        .entries
        .iter()
        .any(|entry| entry.path == "nested/tracked.txt"));
    assert_linked_return_metadata(&fixture, &main_before);
    // This separate local addition must survive the three-way return merge.
    write(&fixture.linked, "local-only.txt", "local disjoint bytes\n");
    let before_apply = metadata_bytes(&fixture.linked);
    let review = sdk
        .reconcile(base.reference(), &fixture.linked, returned.reference())
        .unwrap();
    assert_eq!(review.status, ReconciliationStatus::StagedForReview);
    assert!(review.conflicts.is_empty());
    assert_eq!(metadata_bytes(&fixture.linked), before_apply);
    assert_linked_return_metadata(&fixture, &main_before);
    let options = ApplyOptions {
        quiescent: true,
        expected_fingerprint: review.expected_fingerprint.clone(),
    };
    let receipt = sdk.apply(&review.reference, options.clone()).unwrap();
    assert_eq!(
        fs::read(fixture.linked.join("tracked.txt")).unwrap(),
        b"remote returned working bytes\n"
    );
    assert!(!fixture.linked.join("nested/tracked.txt").exists());
    assert_eq!(
        fs::read(fixture.linked.join("remote-new.txt")).unwrap(),
        b"remote created bytes\n"
    );
    assert_eq!(
        fs::read(fixture.linked.join("local-only.txt")).unwrap(),
        b"local disjoint bytes\n"
    );
    assert_eq!(
        fs::read(fixture.linked.join("selected-untracked.txt")).unwrap(),
        b"selected untracked bytes\n"
    );
    assert_linked_return_metadata(&fixture, &main_before);
    write(&fixture.linked, "tracked.txt", "later user edit\n");
    let after_edit = metadata_bytes(&fixture.linked);
    let retry = sdk.apply(&review.reference, options).unwrap();
    assert_eq!(retry.recovery, receipt.recovery);
    assert_eq!(retry.checkpoint, receipt.checkpoint);
    assert_eq!(metadata_bytes(&fixture.linked), after_edit);
    assert_linked_return_metadata(&fixture, &main_before);
    sdk.rollback(&receipt.recovery, true).unwrap();
    assert_eq!(metadata_bytes(&fixture.linked), before_apply);
    assert_linked_return_metadata(&fixture, &main_before);
    assert!(matches!(
        sdk.inspect_apply(&review.reference).unwrap(),
        ApplyState::RolledBack { .. }
    ));
}

#[test]
fn linked_worktree_return_requires_quiescence_and_unchanged_selected_destination() {
    let fixture = LinkedCheckpointFixture::new();
    prepare_linked_return(&fixture);
    let main_before = metadata_bytes(&fixture.main);
    let sdk = engine(fixture.temporary.path());
    let base = sdk
        .capture(&fixture.linked, CaptureOptions::default())
        .unwrap();
    let remote = linked_return_remote(&sdk, &fixture, base.reference());
    write(&remote, "tracked.txt", "remote returned bytes\n");
    let returned = capture_linked_return(&sdk, &remote);
    let before_review = metadata_bytes(&fixture.linked);
    let review = sdk
        .reconcile(base.reference(), &fixture.linked, returned.reference())
        .unwrap();
    assert_eq!(review.status, ReconciliationStatus::StagedForReview);
    assert_eq!(metadata_bytes(&fixture.linked), before_review);
    assert_linked_return_metadata(&fixture, &main_before);
    assert!(sdk
        .apply(
            &review.reference,
            ApplyOptions {
                quiescent: false,
                expected_fingerprint: review.expected_fingerprint.clone(),
            }
        )
        .is_err());
    assert_eq!(metadata_bytes(&fixture.linked), before_review);
    assert_linked_return_metadata(&fixture, &main_before);
    assert!(matches!(
        sdk.inspect_apply(&review.reference).unwrap(),
        ApplyState::NotStarted
    ));
    write(
        &fixture.linked,
        "tracked.txt",
        "new local edit after review\n",
    );
    let after_edit = metadata_bytes(&fixture.linked);
    assert!(sdk
        .apply(
            &review.reference,
            ApplyOptions {
                quiescent: true,
                expected_fingerprint: review.expected_fingerprint.clone(),
            }
        )
        .is_err());
    assert_eq!(metadata_bytes(&fixture.linked), after_edit);
    assert_linked_return_metadata(&fixture, &main_before);
    assert!(matches!(
        sdk.inspect_apply(&review.reference).unwrap(),
        ApplyState::NotStarted
    ));
}

#[test]
fn linked_worktree_return_conflicts_retain_both_sides_without_source_or_git_writes() {
    let fixture = LinkedCheckpointFixture::new();
    prepare_linked_return(&fixture);
    let main_before = metadata_bytes(&fixture.main);
    let sdk = engine(fixture.temporary.path());
    let base = sdk
        .capture(&fixture.linked, CaptureOptions::default())
        .unwrap();
    let remote = linked_return_remote(&sdk, &fixture, base.reference());
    write(&remote, "tracked.txt", "remote conflicting bytes\n");
    let returned = capture_linked_return(&sdk, &remote);
    write(&fixture.linked, "tracked.txt", "local conflicting bytes\n");
    let before_review = metadata_bytes(&fixture.linked);
    let review = sdk
        .reconcile(base.reference(), &fixture.linked, returned.reference())
        .unwrap();
    assert_eq!(review.status, ReconciliationStatus::Conflicted);
    assert!(review.result.is_none());
    let conflict = review
        .conflicts
        .iter()
        .find(|record| record.path == "tracked.txt")
        .unwrap();
    assert_eq!(
        fs::read(&conflict.base.as_ref().unwrap().preserved_path).unwrap(),
        b"unstaged working bytes\n"
    );
    assert_eq!(
        fs::read(&conflict.local.as_ref().unwrap().preserved_path).unwrap(),
        b"local conflicting bytes\n"
    );
    assert_eq!(
        fs::read(&conflict.remote.as_ref().unwrap().preserved_path).unwrap(),
        b"remote conflicting bytes\n"
    );
    assert_eq!(metadata_bytes(&fixture.linked), before_review);
    assert_linked_return_metadata(&fixture, &main_before);
    assert!(sdk
        .apply(
            &review.reference,
            ApplyOptions {
                quiescent: true,
                expected_fingerprint: review.expected_fingerprint.clone(),
            }
        )
        .is_err());
    assert_eq!(metadata_bytes(&fixture.linked), before_review);
    assert_linked_return_metadata(&fixture, &main_before);
    assert!(sdk.accept_resolution(&review.reference, &[]).is_err());
    assert_eq!(metadata_bytes(&fixture.linked), before_review);
    assert_linked_return_metadata(&fixture, &main_before);
    assert!(matches!(
        sdk.inspect_apply(&review.reference).unwrap(),
        ApplyState::NotStarted
    ));
}
