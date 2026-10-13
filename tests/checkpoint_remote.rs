use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::Command;
use workspace_sdk::checkpoint::*;
use workspace_sdk::{OperationId, TreeTransfer};

fn transfer(id: &str) -> TreeTransfer {
    TreeTransfer {
        id: OperationId::new(id).unwrap(),
    }
}

fn put(root: &Path, path: &str, contents: impl AsRef<[u8]>) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, contents).unwrap();
}

fn git(root: &Path, args: &[&str]) {
    let result = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn native_export_and_checkpoint_capture_preserve_tracked_ignored_binary_modes_and_symlinks() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("remote");
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "--quiet"]);
    put(&root, "tracked.txt", "keep tracked");
    put(&root, ".env", "tracked secret");
    put(&root, "omit.txt", "ignore tracked");
    git(&root, &["add", "."]);
    put(&root, ".gitignore", "tracked.txt\nignored.txt\n");
    put(&root, ".workspaceignore", "omit.txt\n");
    put(&root, "ignored.txt", "ignore");
    put(&root, "blob.bin", [0, 255, 128, 0]);
    put(&root, "tool.sh", "#!/bin/sh\ntrue\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root.join("tool.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("blob.bin", root.join("link")).unwrap();
    }
    let index = fs::read(root.join(".git/index")).unwrap();
    let archive = root.join(".workspace-sdk/transfers/one/tree.json");
    let output = export_remote_tree(&root, &transfer("one")).unwrap();
    assert!(output.files >= 4);
    let bytes = fs::read(&archive).unwrap();
    assert_eq!(output.bytes, bytes.len() as u64);
    assert_eq!(output.sha256, format!("{:x}", Sha256::digest(&bytes)));
    let engine = GitCheckpointEngine::new("git", temp.path().join("state")).unwrap();
    let checkpoint = engine
        .capture_archive(&bytes, CaptureOptions::default())
        .unwrap();
    let paths: Vec<_> = checkpoint
        .manifest
        .entries
        .iter()
        .map(|e| e.path.as_str())
        .collect();
    assert!(paths.contains(&"tracked.txt"));
    assert!(!paths.contains(&"ignored.txt"));
    assert!(!paths.contains(&"omit.txt"));
    assert!(!paths.contains(&".env"));
    assert_eq!(index, fs::read(root.join(".git/index")).unwrap());
    let local = temp.path().join("local");
    engine.restore(checkpoint.reference(), &local).unwrap();
    assert_eq!(fs::read(local.join("blob.bin")).unwrap(), [0, 255, 128, 0]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            fs::metadata(local.join("tool.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0
        );
        assert_eq!(
            fs::read_link(local.join("link")).unwrap(),
            Path::new("blob.bin")
        );
    }
}

#[test]
fn native_archive_import_and_reexport_keep_selection_with_safe_retries() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir(&source).unwrap();
    let target = temp.path().join("target-root");
    fs::create_dir(&target).unwrap();
    git(&source, &["init", "--quiet"]);
    put(&source, "tracked.txt", "preserve");
    git(&source, &["add", "."]);
    put(&source, ".gitignore", "tracked.txt\n");
    let engine = GitCheckpointEngine::new("git", temp.path().join("state")).unwrap();
    let checkpoint = engine.capture(&source, CaptureOptions::default()).unwrap();
    let bytes = engine
        .export_checkpoint_archive(checkpoint.reference())
        .unwrap();
    let archive = target.join(".workspace-sdk/transfers/import-one/tree.json");
    fs::create_dir_all(archive.parent().unwrap()).unwrap();
    fs::write(&archive, &bytes).unwrap();
    import_remote_tree(&target, &transfer("import-one")).unwrap();
    import_remote_tree(&target, &transfer("import-one")).unwrap();
    assert_eq!(
        fs::read_to_string(target.join("tracked.txt")).unwrap(),
        "preserve"
    );
    let reexport = target.join(".workspace-sdk/transfers/export-one/tree.json");
    export_remote_tree(&target, &transfer("export-one")).unwrap();
    let returned = engine
        .capture_archive(&fs::read(reexport).unwrap(), CaptureOptions::default())
        .unwrap();
    assert_eq!(
        checkpoint.manifest.tree_fingerprint,
        returned.manifest.tree_fingerprint
    );
    put(&target, "new.txt", "new user file");
    assert!(import_remote_tree(&target, &transfer("import-one")).is_err());
    fs::remove_file(target.join("new.txt")).unwrap();
    fs::remove_file(target.join("tracked.txt")).unwrap();
    assert!(import_remote_tree(&target, &transfer("import-one")).is_err());
    assert!(!target.join("tracked.txt").exists());
    put(&target, "tracked.txt", "user edit");
    assert!(import_remote_tree(&target, &transfer("import-one")).is_err());
    assert_eq!(
        fs::read_to_string(target.join("tracked.txt")).unwrap(),
        "user edit"
    );
}

#[test]
fn native_import_rejects_malicious_paths_and_symlink_transfer_ancestors() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir(&source).unwrap();
    put(&source, "good", "content");
    let engine = GitCheckpointEngine::new("git", temp.path().join("state")).unwrap();
    let bytes = engine.export_tree_archive(&source).unwrap();
    let mut archive: TreeArchive = serde_json::from_slice(&bytes).unwrap();
    let target = temp.path().join("target-root");
    fs::create_dir(&target).unwrap();
    let path = target.join(".workspace-sdk/transfers/malicious/tree.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    archive.entries[0].file.path = "../outside".into();
    fs::write(&path, serde_json::to_vec(&archive).unwrap()).unwrap();
    assert!(import_remote_tree(&target, &transfer("malicious")).is_err());
    assert!(!target.join("good").exists());
    assert!(!temp.path().join("outside").exists());
    #[cfg(unix)]
    {
        let linked = temp.path().join("linked-root");
        fs::create_dir(&linked).unwrap();
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, linked.join(".workspace-sdk")).unwrap();
        assert!(export_remote_tree(&linked, &transfer("test")).is_err());
        assert!(fs::read_dir(&outside).unwrap().next().is_none());
    }
}

#[test]
fn native_import_resumes_matching_pending_files_and_rejects_modified_partial_files() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    fs::create_dir(&source).unwrap();
    put(&source, "a.txt", "first");
    put(&source, "b.txt", "second");
    let engine = GitCheckpointEngine::new("git", temporary.path().join("state")).unwrap();
    let bytes = engine.export_tree_archive(&source).unwrap();
    for changed in [false, true] {
        let root = temporary
            .path()
            .join(if changed { "changed" } else { "matching" });
        fs::create_dir(&root).unwrap();
        let archive = root.join(".workspace-sdk/transfers/pending/tree.json");
        fs::create_dir_all(archive.parent().unwrap()).unwrap();
        fs::write(&archive, &bytes).unwrap();
        put(
            &root,
            "a.txt",
            if changed { "user changed it" } else { "first" },
        );
        let receipt = serde_json::json!({"version":1,"status":"pending","sha256":format!("{:x}",Sha256::digest(&bytes)),"files":2,"selected_paths":["a.txt","b.txt"]});
        put(
            &root,
            ".workspace-sdk/checkpoint-import.json",
            serde_json::to_vec(&receipt).unwrap(),
        );
        assert!(export_remote_tree(&root, &transfer("premature-export")).is_err());
        let output = import_remote_tree(&root, &transfer("pending"));
        if changed {
            assert!(output.is_err());
            assert_eq!(
                fs::read_to_string(root.join("a.txt")).unwrap(),
                "user changed it"
            );
            assert!(!root.join("b.txt").exists());
        } else {
            output.unwrap();
            assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "first");
            assert_eq!(fs::read_to_string(root.join("b.txt")).unwrap(), "second");
            import_remote_tree(&root, &transfer("pending")).unwrap();
        }
    }
}

#[test]
fn transfer_cleanup_is_scoped_idempotent_and_preserves_project_and_import_receipt() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    fs::create_dir(&root).unwrap();
    put(&root, "user.txt", "user file");
    export_remote_tree(&root, &transfer("first")).unwrap();
    export_remote_tree(&root, &transfer("second")).unwrap();
    put(
        &root,
        ".workspace-sdk/checkpoint-import.json",
        "receipt stays",
    );
    cleanup_transfer(&root, &transfer("first")).unwrap();
    cleanup_transfer(&root, &transfer("first")).unwrap();
    assert!(!root
        .join(transfer("first").relative_directory().as_str())
        .exists());
    assert!(root
        .join(transfer("second").relative_archive_path().as_str())
        .is_file());
    assert_eq!(
        fs::read_to_string(root.join("user.txt")).unwrap(),
        "user file"
    );
    assert_eq!(
        fs::read_to_string(root.join(".workspace-sdk/checkpoint-import.json")).unwrap(),
        "receipt stays"
    );
    #[cfg(unix)]
    {
        let outside = temporary.path().join("outside");
        fs::create_dir(&outside).unwrap();
        put(&outside, "keep", "outside stays");
        let linked = root.join(transfer("linked").relative_directory().as_str());
        std::os::unix::fs::symlink(&outside, linked).unwrap();
        assert!(cleanup_transfer(&root, &transfer("linked")).is_err());
        assert_eq!(
            fs::read_to_string(outside.join("keep")).unwrap(),
            "outside stays"
        );
    }
}

#[test]
fn native_import_validates_all_archive_entries_before_publishing_any_file() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    fs::create_dir(&source).unwrap();
    put(&source, "a-good", "content");
    put(&source, "z-bad", "content");
    let engine = GitCheckpointEngine::new("git", temporary.path().join("state")).unwrap();
    let bytes = engine.export_tree_archive(&source).unwrap();
    for corruption in [
        "checksum",
        "excluded",
        "overlap",
        "case-collision",
        "symlink-escape",
    ] {
        let mut archive: TreeArchive = serde_json::from_slice(&bytes).unwrap();
        match corruption {
            "checksum" => archive.entries[1].file.sha256 = "0".repeat(64),
            "excluded" => archive.entries[1].file.path = ".env".into(),
            "overlap" => archive.entries[1].file.path = "a-good/child".into(),
            "case-collision" => archive.entries[1].file.path = "A-GOOD".into(),
            "symlink-escape" => {
                use base64::Engine;
                let target = b"../outside";
                let entry = &mut archive.entries[1];
                entry.file.kind = EntryKind::Symlink;
                entry.file.mode = 120000;
                entry.file.size = target.len() as u64;
                entry.file.sha256 = format!("{:x}", Sha256::digest(target));
                entry.data_base64 = base64::engine::general_purpose::STANDARD.encode(target);
            }
            _ => unreachable!(),
        }
        let root = temporary.path().join(corruption);
        fs::create_dir(&root).unwrap();
        put(
            &root,
            transfer("invalid").relative_archive_path().as_str(),
            serde_json::to_vec(&archive).unwrap(),
        );
        assert!(
            import_remote_tree(&root, &transfer("invalid")).is_err(),
            "{corruption}"
        );
        assert!(!root.join("a-good").exists());
        assert!(!root.join(".workspace-sdk/checkpoint-import.json").exists());
    }
}

#[cfg(unix)]
#[test]
fn native_export_reports_special_files_that_git_does_not_list() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("source");
    fs::create_dir(&root).unwrap();
    let path = root.join("pipe");
    let result = Command::new("mkfifo").arg(&path).status().unwrap();
    assert!(result.success());
    let error = export_remote_tree(&root, &transfer("special")).unwrap_err();
    assert!(error.to_string().contains("unsupported filesystem feature"));
    let archive = root.join(transfer("fifo-archive").relative_archive_path().as_str());
    fs::create_dir_all(archive.parent().unwrap()).unwrap();
    assert!(Command::new("mkfifo")
        .arg(&archive)
        .status()
        .unwrap()
        .success());
    assert!(import_remote_tree(&root, &transfer("fifo-archive")).is_err());
}
