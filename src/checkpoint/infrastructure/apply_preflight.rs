//! Reject collisions with uncaptured data before apply creates a recovery journal.
use super::file_tree::Tree;
use crate::{checkpoint::CheckpointLimits, Error, Result};
use std::{fs, io::ErrorKind, path::Path};

pub(super) fn preflight(
    root: &Path,
    before: &Tree,
    after: &Tree,
    limits: &CheckpointLimits,
) -> Result<()> {
    let mut visited = 0usize;
    for path in after.files.keys() {
        let mut relative = std::path::PathBuf::new();
        for component in Path::new(path).components() {
            visit(&mut visited, limits)?;
            relative.push(component);
            let metadata = match fs::symlink_metadata(root.join(&relative)) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == ErrorKind::NotFound => break,
                Err(error) => return Err(error.into()),
            };
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                if relative == Path::new(path) {
                    if !selected_directory(before, &relative)? {
                        return Err(collision());
                    }
                    let mut pending = vec![root.join(&relative)];
                    while let Some(directory) = pending.pop() {
                        for child in fs::read_dir(directory)? {
                            let child = child?;
                            visit(&mut visited, limits)?;
                            let path = child.path();
                            let child_relative = path.strip_prefix(root).map_err(|_| {
                                Error::State("apply preflight root mismatch".into())
                            })?;
                            let child_metadata = fs::symlink_metadata(&path)?;
                            if child_metadata.is_dir() && !child_metadata.file_type().is_symlink() {
                                if !selected_directory(before, child_relative)? {
                                    return Err(collision());
                                }
                                pending.push(path);
                            } else if !before
                                .files
                                .contains_key(child_relative.to_str().ok_or_else(collision)?)
                            {
                                return Err(collision());
                            }
                        }
                    }
                }
            } else {
                if !before
                    .files
                    .contains_key(relative.to_str().ok_or_else(collision)?)
                {
                    return Err(collision());
                }
                // A selected file/symlink ancestor will be removed before materializing
                // its replacement subtree. Do not follow it during preflight.
                break;
            }
        }
    }
    Ok(())
}
fn selected_directory(before: &Tree, path: &Path) -> Result<bool> {
    let prefix = format!("{}/", path.to_str().ok_or_else(collision)?);
    // Query the existing ordered catalogue instead of retaining every ancestor:
    // ancestor strings can otherwise grow quadratically for deeply nested paths.
    Ok(before
        .files
        .range(prefix.clone()..)
        .next()
        .is_some_and(|(path, _)| path.starts_with(&prefix)))
}
fn visit(visited: &mut usize, limits: &CheckpointLimits) -> Result<()> {
    *visited += 1;
    if *visited > limits.max_files.saturating_mul(20).max(1000) {
        return Err(Error::InvalidInput(
            "apply collision traversal exceeds limit".into(),
        ));
    }
    Ok(())
}
fn collision() -> Error {
    Error::Conflict(
        "apply destination collides with unselected data; preserve it and stage a new review"
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint::infrastructure::file_tree::TreeFile;
    use crate::checkpoint::{EntryKind, FileEntry};
    use std::collections::BTreeMap;
    fn tree(paths: &[&str]) -> Tree {
        Tree {
            files: paths
                .iter()
                .map(|path| {
                    (
                        (*path).to_owned(),
                        TreeFile {
                            entry: FileEntry {
                                path: (*path).to_owned(),
                                kind: EntryKind::File,
                                mode: 100644,
                                size: 0,
                                sha256: String::new(),
                            },
                            bytes: vec![],
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>(),
            excluded: vec![],
        }
    }
    #[test]
    fn never_admits_unselected_file_symlink_or_empty_directory_collisions() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path();
        fs::write(root.join("file"), b"human").unwrap();
        fs::create_dir(root.join("empty")).unwrap();
        std::os::unix::fs::symlink("file", root.join("link")).unwrap();
        for candidate in ["file", "empty", "link", "link/child"] {
            assert!(matches!(
                preflight(
                    root,
                    &tree(&[]),
                    &tree(&[candidate]),
                    &CheckpointLimits::default()
                ),
                Err(Error::Conflict(_))
            ));
        }
        assert_eq!(fs::read(root.join("file")).unwrap(), b"human");
    }
    #[test]
    fn selected_directory_transition_rejects_excluded_children_without_mutation() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir(root.join("dir")).unwrap();
        fs::write(root.join("dir/selected"), []).unwrap();
        let before = tree(&["dir/selected"]);
        let after = tree(&["dir"]);
        assert!(preflight(root, &before, &after, &CheckpointLimits::default()).is_ok());
        fs::write(root.join("dir/.env"), b"synthetic excluded").unwrap();
        assert!(preflight(root, &before, &after, &CheckpointLimits::default()).is_err());
        assert!(root.join("dir/selected").exists());
        assert!(root.join("dir/.env").exists());
    }
    #[test]
    fn deep_component_checks_are_bounded_before_any_mutation() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = format!("{}/leaf", vec!["d"; 1001].join("/"));
        fs::create_dir_all(temp.path().join(Path::new(&path).parent().unwrap())).unwrap();
        fs::write(temp.path().join(&path), b"preserved").unwrap();
        let limits = CheckpointLimits {
            max_files: 1,
            ..Default::default()
        };
        assert!(matches!(
            preflight(temp.path(), &tree(&[&path]), &tree(&[&path]), &limits),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(fs::read(temp.path().join(path)).unwrap(), b"preserved");
    }
}
