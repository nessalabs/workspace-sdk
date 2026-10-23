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
fn apply_file_directory_transition_and_excluded_collision_are_safe() {
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
    assert!(failure.contains("unselected data"), "{failure}");
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
