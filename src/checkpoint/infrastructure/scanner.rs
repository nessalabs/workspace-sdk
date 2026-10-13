use super::file_tree::*;
use super::filesystem::*;
use super::path_policy::*;
use crate::checkpoint::domain::*;
use crate::{Error, Result};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{ErrorKind, Result as IoResult};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub(crate) fn scan(
    root: &Path,
    tracked: &BTreeSet<String>,
    limits: &CheckpointLimits,
) -> Result<Tree> {
    if fs::symlink_metadata(root)?.file_type().is_symlink() || !root.is_dir() {
        return Err(Error::InvalidInput(
            "checkpoint root must be a real directory".into(),
        ));
    }
    let mut tree = Tree {
        files: BTreeMap::new(),
        excluded: vec![],
    };
    scan_dir(root, root, tracked, limits, &[], &[], &mut tree, &mut 0)?;
    check_entries(&tree.entries(), limits)?;
    Ok(tree)
}

fn load_ignore(directory: &Path, name: &str) -> Result<Option<Gitignore>> {
    let path = directory.join(name);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(Error::InvalidInput(format!(
            "ignore file must be a regular file: {}",
            path.display()
        )));
    }
    if metadata.len() > 1024 * 1024 {
        return Err(Error::InvalidInput("ignore file exceeds size limit".into()));
    }
    let bytes = read_regular(&path, 1024 * 1024)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| Error::InvalidInput("ignore file must be UTF-8".into()))?;
    let mut builder = GitignoreBuilder::new(directory);
    for line in text.lines() {
        builder
            .add_line(Some(path.clone()), line)
            .map_err(|e| Error::InvalidInput(format!("invalid ignore pattern: {e}")))?;
    }
    Ok(Some(builder.build().map_err(|e| {
        Error::InvalidInput(format!("invalid ignore file: {e}"))
    })?))
}

fn ignored(rules: &[Gitignore], path: &Path, is_directory: bool) -> bool {
    let mut excluded = false;
    for rule in rules {
        let matched = rule.matched_path_or_any_parents(path, is_directory);
        if matched.is_ignore() {
            excluded = true;
        } else if matched.is_whitelist() {
            excluded = false;
        }
    }
    excluded
}

#[allow(clippy::too_many_arguments)]
fn scan_dir(
    root: &Path,
    directory: &Path,
    tracked: &BTreeSet<String>,
    limits: &CheckpointLimits,
    git_rules: &[Gitignore],
    workspace_rules: &[Gitignore],
    tree: &mut Tree,
    visited: &mut usize,
) -> Result<()> {
    let mut git_rules = git_rules.to_vec();
    let mut workspace_rules = workspace_rules.to_vec();
    if let Some(rules) = load_ignore(directory, ".gitignore")? {
        git_rules.push(rules);
    }
    if let Some(rules) = load_ignore(directory, ".workspaceignore")? {
        workspace_rules.push(rules);
    }
    let mut children = fs::read_dir(directory)?.collect::<IoResult<Vec<_>>>()?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        *visited += 1;
        if *visited > limits.max_files.saturating_mul(20).max(1000) {
            return Err(Error::InvalidInput(
                "checkpoint traversal exceeds entry limit".into(),
            ));
        }
        let path = child.path();
        let name = path
            .strip_prefix(root)
            .unwrap()
            .to_str()
            .ok_or_else(|| Error::Unsupported("non-UTF-8 file paths are not portable".into()))?
            .to_owned();
        relative_path(&name)?;
        let metadata = fs::symlink_metadata(&path)?;
        let is_directory = metadata.is_dir();
        let tracked_here = tracked.contains(&name)
            || (is_directory && tracked.iter().any(|p| p.starts_with(&format!("{name}/"))));
        let reason = if mandatory_exclusion(&name) {
            Some("secret, control state, or dependency/build cache")
        } else if ignored(&workspace_rules, &path, is_directory) {
            Some(".workspaceignore")
        } else if !tracked_here && ignored(&git_rules, &path, is_directory) {
            Some(".gitignore")
        } else {
            None
        };
        if let Some(reason) = reason {
            tree.excluded.push(ExcludedPath {
                path: name,
                reason: reason.into(),
            });
            continue;
        }
        if is_directory {
            scan_dir(
                root,
                &path,
                tracked,
                limits,
                &git_rules,
                &workspace_rules,
                tree,
                visited,
            )?;
            continue;
        }
        let (kind, mode, bytes) = if metadata.file_type().is_symlink() {
            let target = fs::read_link(&path)?;
            let bytes = target
                .to_str()
                .ok_or_else(|| Error::Unsupported("non-UTF-8 symlink target".into()))?
                .as_bytes()
                .to_vec();
            validate_symlink(&name, &bytes)?;
            (EntryKind::Symlink, 120000, bytes)
        } else if metadata.is_file() {
            #[cfg(unix)]
            let executable = { metadata.permissions().mode() & 0o111 != 0 };
            #[cfg(not(unix))]
            let executable = false;
            (
                EntryKind::File,
                if executable { 100755 } else { 100644 },
                read_regular(&path, limits.max_file_bytes)?,
            )
        } else {
            return Err(Error::Unsupported(format!(
                "unsupported filesystem entry: {name}"
            )));
        };
        if bytes.starts_with(b"version https://git-lfs.github.com/spec/v1\n") {
            return Err(Error::Unsupported(format!(
                "unresolved Git LFS pointer: {name}"
            )));
        }
        tree.files.insert(
            name.clone(),
            TreeFile {
                entry: FileEntry {
                    path: name,
                    kind,
                    mode,
                    size: bytes.len() as u64,
                    sha256: digest(&bytes),
                },
                bytes,
            },
        );
        if tree.files.len() > limits.max_files
            || tree.files.values().map(|f| f.entry.size).sum::<u64>() > limits.max_total_bytes
        {
            return Err(Error::InvalidInput(
                "checkpoint content exceeds size limit".into(),
            ));
        }
    }
    Ok(())
}
