use super::file_tree::*;
use crate::checkpoint::domain::*;
use crate::{Error, Result};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::str::from_utf8;

pub(crate) fn valid_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 80
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
    {
        return Err(Error::InvalidInput(
            "invalid checkpoint/reconciliation identifier".into(),
        ));
    }
    Ok(())
}

pub(crate) fn relative_path(value: &str) -> Result<PathBuf> {
    if value.is_empty()
        || value.len() > 4096
        || value.contains(['\\', ':'])
        || value.chars().any(char::is_control)
    {
        return Err(Error::InvalidInput(format!(
            "nonportable relative path: {value:?}"
        )));
    }
    let path = Path::new(value);
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
        || path.to_str() != Some(value)
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(Error::InvalidInput(format!(
            "unsafe relative path: {value:?}"
        )));
    }
    Ok(path.to_owned())
}

pub(crate) fn mandatory_exclusion(path: &str) -> bool {
    path.split('/').any(|part| {
        let name = part.to_ascii_lowercase();
        matches!(
            name.as_str(),
            ".git"
                | ".hg"
                | ".svn"
                | ".workspace-sdk"
                | ".workspace"
                | ".codex"
                | ".claude"
                | ".opencode"
                | ".ssh"
                | ".aws"
                | ".azure"
                | ".kube"
                | ".gnupg"
                | ".npm"
                | ".cache"
                | ".venv"
                | "venv"
                | "node_modules"
                | "target"
                | "dist"
                | "build"
                | "__pycache__"
                | ".next"
                | ".nuxt"
                | ".turbo"
                | "coverage"
                | "auth.json"
                | "credentials.json"
                | ".npmrc"
                | ".pypirc"
                | "id_rsa"
                | "id_dsa"
                | "id_ecdsa"
                | "id_ed25519"
        ) || name == ".env"
            || name.starts_with(".env.")
            || name.ends_with(".pem")
            || name.ends_with(".key")
            || name.ends_with(".p12")
            || name.ends_with(".pfx")
    })
}

pub(crate) fn validate_symlink(path: &str, target: &[u8]) -> Result<()> {
    let text =
        from_utf8(target).map_err(|_| Error::Unsupported("non-UTF-8 symlink target".into()))?;
    if text.is_empty()
        || text.contains(['\\', ':'])
        || text.chars().any(char::is_control)
        || Path::new(text).is_absolute()
    {
        return Err(Error::InvalidInput(format!(
            "unsafe symlink target for {path}"
        )));
    }
    let mut segments: Vec<&str> = path.split('/').collect();
    segments.pop();
    for part in Path::new(text).components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if segments.pop().is_none() {
                    return Err(Error::InvalidInput(format!(
                        "symlink escapes snapshot: {path}"
                    )));
                }
            }
            Component::Normal(p) => segments.push(
                p.to_str()
                    .ok_or_else(|| Error::Unsupported("non-UTF-8 symlink".into()))?,
            ),
            _ => return Err(Error::InvalidInput(format!("unsafe symlink: {path}"))),
        }
    }
    if mandatory_exclusion(&segments.join("/")) {
        return Err(Error::InvalidInput(format!(
            "symlink points into excluded state: {path}"
        )));
    }
    Ok(())
}

pub(crate) fn check_entries(entries: &[FileEntry], limits: &CheckpointLimits) -> Result<()> {
    if entries.len() > limits.max_files {
        return Err(Error::InvalidInput(
            "checkpoint file count exceeds limit".into(),
        ));
    }
    let mut paths = BTreeSet::new();
    let mut portable_components = BTreeMap::new();
    let mut total = 0u64;
    for entry in entries {
        relative_path(&entry.path)?;
        if mandatory_exclusion(&entry.path) {
            return Err(Error::InvalidInput(format!(
                "checkpoint contains excluded path: {}",
                entry.path
            )));
        }
        if !paths.insert(entry.path.clone()) {
            return Err(Error::InvalidInput("duplicate checkpoint path".into()));
        }
        let mut prefix = String::new();
        for component in entry.path.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            if portable_components
                .insert(prefix.to_lowercase(), prefix.clone())
                .is_some_and(|previous| previous != prefix)
            {
                return Err(Error::InvalidInput(
                    "checkpoint paths collide on a case-insensitive destination".into(),
                ));
            }
        }
        if !matches!(
            (entry.kind, entry.mode),
            (EntryKind::File, 100644 | 100755) | (EntryKind::Symlink, 120000)
        ) {
            return Err(Error::InvalidInput("unsupported file mode".into()));
        }
        if entry.sha256.len() != 64
            || !entry
                .sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(Error::InvalidInput("invalid file checksum".into()));
        }
        total = total
            .checked_add(entry.size)
            .ok_or_else(|| Error::InvalidInput("checkpoint size overflow".into()))?;
        if entry.size > limits.max_file_bytes || total > limits.max_total_bytes {
            return Err(Error::InvalidInput(
                "checkpoint content exceeds size limit".into(),
            ));
        }
    }
    for path in &paths {
        let mut parent = Path::new(path).parent();
        while let Some(p) = parent {
            if paths.contains(p.to_str().unwrap_or("")) {
                return Err(Error::InvalidInput(
                    "checkpoint file overlaps parent path".into(),
                ));
            }
            parent = p.parent();
        }
    }
    Ok(())
}

pub(crate) fn validate_tree_symlinks(tree: &Tree) -> Result<()> {
    for file in tree
        .files
        .values()
        .filter(|file| file.entry.kind == EntryKind::Symlink)
    {
        validate_symlink(&file.entry.path, &file.bytes)?;
        let mut resolved: Vec<String> = file.entry.path.split('/').map(str::to_owned).collect();
        resolved.pop();
        let mut pending: VecDeque<String> = from_utf8(&file.bytes)
            .unwrap()
            .split('/')
            .map(str::to_owned)
            .collect();
        let mut followed = 0;
        while let Some(part) = pending.pop_front() {
            match part.as_str() {
                "" | "." => continue,
                ".." => {
                    if resolved.pop().is_none() {
                        return Err(Error::InvalidInput(format!(
                            "symlink chain escapes snapshot: {}",
                            file.entry.path
                        )));
                    }
                }
                _ => {
                    resolved.push(part);
                    if let Some(target) = tree.files.get(&resolved.join("/")) {
                        if target.entry.kind == EntryKind::Symlink {
                            followed += 1;
                            if followed > 40 {
                                return Err(Error::InvalidInput(
                                    "symlink chain is cyclic or exceeds limit".into(),
                                ));
                            }
                            validate_symlink(&target.entry.path, &target.bytes)?;
                            resolved.pop();
                            for item in from_utf8(&target.bytes).unwrap().split('/').rev() {
                                pending.push_front(item.into());
                            }
                        }
                    }
                }
            }
            if mandatory_exclusion(&resolved.join("/")) {
                return Err(Error::InvalidInput(
                    "symlink chain points into excluded state".into(),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_paths_never_address_parents_windows_drives_or_control_state() {
        for path in [
            "../outside",
            "/absolute",
            "a/../b",
            "C:\\key",
            "double//slash",
            "a\nfile",
        ] {
            assert!(relative_path(path).is_err(), "{path:?}");
        }
        for path in [
            ".git/config",
            "nested/.env.production",
            "keys/server.pem",
            "node_modules/pkg/index.js",
        ] {
            assert!(mandatory_exclusion(path), "{path}");
        }
        assert!(!mandatory_exclusion("src/environment.rs"));
    }

    #[test]
    fn symlinks_can_reference_a_sibling_but_not_secrets_or_parent_of_root() {
        assert!(validate_symlink("src/link", b"../README.md").is_ok());
        assert!(validate_symlink("link", b"../outside").is_err());
        assert!(validate_symlink("src/link", b"../.env").is_err());
    }
}
