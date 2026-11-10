use super::file_tree::*;
use super::filesystem::*;
use super::path_policy::*;
use crate::checkpoint::domain::*;
use crate::{Error, Result};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
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
    scan_dir(
        root,
        root,
        tracked,
        limits,
        &[],
        &[],
        &mut tree,
        &mut ScanBudget::default(),
    )?;
    // Recreate sorted-sibling depth-first report order without collecting each
    // directory's entries. Full-path sorting differs for `a/child` and `a-b`.
    tree.excluded
        .sort_by(|a, b| a.path.split('/').cmp(b.path.split('/')));
    check_entries(&tree.entries(), limits)?;
    Ok(tree)
}

#[derive(Default)]
struct ScanBudget {
    visited: usize,
    total_bytes: u64,
}

impl ScanBudget {
    fn visit(&mut self, limits: &CheckpointLimits) -> Result<()> {
        self.visited = self
            .visited
            .checked_add(1)
            .filter(|visited| *visited <= limits.max_files.saturating_mul(20).max(1000))
            .ok_or_else(|| {
                Error::InvalidInput("checkpoint traversal exceeds entry limit".into())
            })?;
        Ok(())
    }

    fn select(&mut self, bytes: u64, limits: &CheckpointLimits) -> Result<()> {
        self.total_bytes = self
            .total_bytes
            .checked_add(bytes)
            .filter(|total| *total <= limits.max_total_bytes)
            .ok_or_else(|| Error::InvalidInput("checkpoint content exceeds size limit".into()))?;
        Ok(())
    }
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
    budget: &mut ScanBudget,
) -> Result<()> {
    let mut git_rules = git_rules.to_vec();
    let mut workspace_rules = workspace_rules.to_vec();
    if let Some(rules) = load_ignore(directory, ".gitignore")? {
        git_rules.push(rules);
    }
    if let Some(rules) = load_ignore(directory, ".workspaceignore")? {
        workspace_rules.push(rules);
    }
    for child in fs::read_dir(directory)? {
        budget.visit(limits)?;
        let path = child?.path();
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
                budget,
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
        budget.select(bytes.len() as u64, limits)?;
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
        if tree.files.len() > limits.max_files {
            return Err(Error::InvalidInput(
                "checkpoint content exceeds size limit".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversal_counter_refuses_overflow_even_with_saturated_entry_allowance() {
        let limits = CheckpointLimits {
            max_files: usize::MAX,
            ..CheckpointLimits::default()
        };
        let mut budget = ScanBudget {
            visited: usize::MAX,
            total_bytes: 0,
        };
        assert!(matches!(budget.visit(&limits), Err(Error::InvalidInput(_))));
    }

    #[test]
    fn selected_byte_counter_refuses_overflow_at_maximum_allowance() {
        let limits = CheckpointLimits {
            max_total_bytes: u64::MAX,
            ..CheckpointLimits::default()
        };
        let mut budget = ScanBudget {
            visited: 0,
            total_bytes: u64::MAX,
        };
        assert!(matches!(
            budget.select(1, &limits),
            Err(Error::InvalidInput(_))
        ));
    }
}
