//! Public capture/archive selection and bound contracts. These tests make no
//! allocation, RSS or speed claim about directory traversal.
use std::{fs, path::Path};
use workspace_sdk::checkpoint::*;

fn put(root: &Path, path: &str, bytes: &[u8]) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn engine(root: &Path, name: &str, limits: CheckpointLimits) -> GitCheckpointEngine {
    GitCheckpointEngine::new("git", root.join(name))
        .unwrap()
        .with_limits(limits)
        .unwrap()
}

#[test]
fn excluded_report_retains_component_depth_first_order_across_creation_orders() {
    let temporary = tempfile::tempdir().unwrap();
    let first = temporary.path().join("first");
    let second = temporary.path().join("second");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    // Component traversal visits a's descendants before a-b, even though raw
    // full-path string ordering places '-' before '/'. Include Unicode siblings.
    let excluded = [
        "a/.env",
        "a/z/.env",
        "a-b/.env",
        "a.b/.env",
        "a0/.env",
        "é/.env",
        "é-/.env",
        "éclair/.env",
    ];
    for path in excluded {
        put(&first, path, b"disposable excluded data");
    }
    for path in excluded.into_iter().rev() {
        put(&second, path, b"disposable excluded data");
    }
    for root in [&first, &second] {
        put(root, "selected/data.bin", &[0, 255, 1, 128]);
        put(root, "a/selected.txt", b"same selected contents\n");
    }
    let sdk = engine(temporary.path(), "state", CheckpointLimits::default());
    let left = sdk.capture(&first, CaptureOptions::default()).unwrap();
    let right = sdk.capture(&second, CaptureOptions::default()).unwrap();
    assert_eq!(
        left.manifest
            .excluded
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        excluded
    );
    assert_eq!(left.manifest.excluded, right.manifest.excluded);
    assert_eq!(left.manifest.entries, right.manifest.entries);
    assert_eq!(
        left.manifest.tree_fingerprint,
        right.manifest.tree_fingerprint
    );
    assert_eq!(
        sdk.export_tree_archive(&first).unwrap(),
        sdk.export_tree_archive(&second).unwrap()
    );
}

#[test]
fn excluded_entries_still_consume_the_positive_traversal_allowance() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    fs::create_dir(&source).unwrap();
    for index in 0..1000 {
        put(&source, &format!(".env.{index:04}"), b"");
    }
    let sdk = engine(
        temporary.path(),
        "state",
        CheckpointLimits {
            max_files: 1,
            ..CheckpointLimits::default()
        },
    );
    let accepted = sdk.capture(&source, CaptureOptions::default()).unwrap();
    assert!(accepted.manifest.entries.is_empty());
    assert_eq!(accepted.manifest.excluded.len(), 1000);
    let prior = sdk.export(accepted.reference()).unwrap().bytes;
    put(&source, ".env.1000", b"");
    let capture_error = sdk.capture(&source, CaptureOptions::default()).unwrap_err();
    assert!(capture_error.to_string().contains("traversal"));
    let archive_error = sdk.export_tree_archive(&source).unwrap_err();
    assert!(archive_error.to_string().contains("traversal"));
    assert_eq!(sdk.export(accepted.reference()).unwrap().bytes, prior);
    assert_eq!(
        fs::read_dir(sdk.state_directory().join("checkpoints"))
            .unwrap()
            .count(),
        1,
        "refused capture must not publish another checkpoint"
    );
}

#[test]
fn aggregate_content_limit_is_exact_across_sibling_directories() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    put(&source, "first/three", b"abc");
    put(&source, "second/four", b"defg");
    let limits = CheckpointLimits {
        max_files: 2,
        max_file_bytes: 4,
        max_total_bytes: 7,
        ..CheckpointLimits::default()
    };
    let accepted = engine(temporary.path(), "accepted", limits.clone());
    let checkpoint = accepted
        .capture(&source, CaptureOptions::default())
        .unwrap();
    assert_eq!(checkpoint.manifest.total_bytes, 7);
    assert_eq!(checkpoint.manifest.entries.len(), 2);
    let archive = accepted.export_tree_archive(&source).unwrap();
    let restored = temporary.path().join("restored");
    accepted.import_tree_archive(&archive, &restored).unwrap();
    assert_eq!(fs::read(restored.join("first/three")).unwrap(), b"abc");
    assert_eq!(fs::read(restored.join("second/four")).unwrap(), b"defg");
    let rejected = engine(
        temporary.path(),
        "rejected",
        CheckpointLimits {
            max_total_bytes: 6,
            ..limits
        },
    );
    assert!(rejected
        .capture(&source, CaptureOptions::default())
        .is_err());
    assert!(rejected.export_tree_archive(&source).is_err());
    assert_eq!(
        fs::read_dir(rejected.state_directory().join("checkpoints"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(fs::read(source.join("first/three")).unwrap(), b"abc");
    assert_eq!(fs::read(source.join("second/four")).unwrap(), b"defg");
}

#[test]
fn selected_file_limit_is_shared_across_sibling_directories() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    put(&source, "first/one", b"a");
    put(&source, "second/two", b"b");
    let sdk = engine(
        temporary.path(),
        "state",
        CheckpointLimits {
            max_files: 2,
            max_file_bytes: 1,
            max_total_bytes: 3,
            ..CheckpointLimits::default()
        },
    );
    let accepted = sdk.capture(&source, CaptureOptions::default()).unwrap();
    assert_eq!(accepted.manifest.entries.len(), 2);
    put(&source, "third/three", b"c");
    assert!(sdk.capture(&source, CaptureOptions::default()).is_err());
    assert!(sdk.export_tree_archive(&source).is_err());
    assert_eq!(
        fs::read_dir(sdk.state_directory().join("checkpoints"))
            .unwrap()
            .count(),
        1
    );
    assert_eq!(fs::read(source.join("third/three")).unwrap(), b"c");
}

#[cfg(unix)]
#[test]
fn aggregate_limit_counts_symlink_target_bytes_with_regular_file_bytes() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    put(&source, "data", b"abc");
    std::os::unix::fs::symlink("data", source.join("link")).unwrap();
    let limits = CheckpointLimits {
        max_files: 2,
        max_file_bytes: 4,
        max_total_bytes: 7,
        ..CheckpointLimits::default()
    };
    let accepted = engine(temporary.path(), "accepted", limits.clone());
    let checkpoint = accepted
        .capture(&source, CaptureOptions::default())
        .unwrap();
    assert_eq!(checkpoint.manifest.total_bytes, 7);
    let archive = accepted.export_tree_archive(&source).unwrap();
    let restored = temporary.path().join("restored");
    accepted.import_tree_archive(&archive, &restored).unwrap();
    assert_eq!(
        fs::read_link(restored.join("link")).unwrap(),
        Path::new("data")
    );
    assert_eq!(fs::read(restored.join("data")).unwrap(), b"abc");
    let rejected = engine(
        temporary.path(),
        "rejected",
        CheckpointLimits {
            max_total_bytes: 6,
            ..limits
        },
    );
    assert!(rejected
        .capture(&source, CaptureOptions::default())
        .is_err());
    assert!(rejected.export_tree_archive(&source).is_err());
    assert_eq!(
        fs::read_link(source.join("link")).unwrap(),
        Path::new("data")
    );
}
