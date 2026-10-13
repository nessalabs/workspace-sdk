//! Lexically validated paths that stay relative across controller and target platforms.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// A validated, portable path relative to a workspace’s project directory.
///
/// This value object uses `/` separators on every host. `.` names the project
/// root; `src/main.rs` names a child. Construction rejects absolute paths,
/// parent traversal, NUL bytes, backslashes, and colons (reserved here to exclude
/// Windows drive syntax). Repeated separators and `.` components are normalized.
///
/// Validation is lexical: it does not read the filesystem. A backend must still
/// resolve symlinks safely before using the path. Keeping the string private
/// ensures construction and deserialization both preserve these rules.
///
/// ```
/// use workspace_sdk::WorkspacePath;
/// let path = WorkspacePath::new("./src//main.rs")?;
/// assert_eq!(path.as_str(), "src/main.rs");
/// assert!(WorkspacePath::new("../outside").is_err());
/// # Ok::<(), workspace_sdk::Error>(())
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct WorkspacePath(String);
impl WorkspacePath {
    /// Validate and normalize a workspace-relative path without touching the filesystem.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_portable_characters(&value)?;
        validate_relative_components(&value)?;

        let normalized = value
            .split('/')
            .filter(|component| !component.is_empty() && *component != ".")
            .collect::<Vec<_>>()
            .join("/");
        Ok(Self(if normalized.is_empty() {
            ".".into()
        } else {
            normalized
        }))
    }
    /// Return the project root, represented by `.`.
    pub fn root() -> Self {
        Self(".".into())
    }
    /// Borrow the normalized path with portable `/` separators.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
// These rules are deliberately independent of the controller's operating system:
// a path accepted on macOS must not become a Windows drive path on another target.
fn validate_portable_characters(value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(Error::InvalidInput(
            "workspace path cannot be empty; use . for the root".into(),
        ));
    }
    if value.contains('\0') {
        return Err(Error::InvalidInput(
            "workspace path cannot contain NUL bytes".into(),
        ));
    }
    if value.contains('\\') {
        return Err(Error::InvalidInput(
            "workspace path must use / separators, not backslashes".into(),
        ));
    }
    if value.contains(':') {
        return Err(Error::InvalidInput(
            "workspace path cannot contain colons or drive prefixes".into(),
        ));
    }
    Ok(())
}

fn validate_relative_components(value: &str) -> Result<()> {
    if value.starts_with('/') {
        return Err(Error::InvalidInput(
            "workspace path must be relative, not absolute".into(),
        ));
    }
    if value.split('/').any(|component| component == "..") {
        return Err(Error::InvalidInput(
            "workspace path cannot traverse a parent directory with ..".into(),
        ));
    }
    Ok(())
}

impl Default for WorkspacePath {
    fn default() -> Self {
        Self::root()
    }
}
impl TryFrom<String> for WorkspacePath {
    type Error = Error;
    fn try_from(v: String) -> Result<Self> {
        Self::new(v)
    }
}
impl From<WorkspacePath> for String {
    fn from(v: WorkspacePath) -> Self {
        v.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_escaping_and_deserialized_paths() {
        for p in [
            "",
            "../x",
            "a/../../b",
            "/etc/passwd",
            "//server/share",
            r"C:\secret",
            "C:relative",
            r"a\b",
            "a\0b",
        ] {
            assert!(WorkspacePath::new(p).is_err());
            assert!(serde_json::from_value::<WorkspacePath>(serde_json::json!(p)).is_err());
        }
    }
    #[test]
    fn accepts_regular_names_that_only_contain_dots() {
        for path in ["a..b", ".config/file", "a/.../b", "résumé notes.txt"] {
            assert_eq!(WorkspacePath::new(path).unwrap().as_str(), path);
        }
    }
    #[test]
    fn explains_which_boundary_was_violated() {
        assert!(WorkspacePath::new("../x")
            .unwrap_err()
            .to_string()
            .contains("parent directory"));
        assert!(WorkspacePath::new("/x")
            .unwrap_err()
            .to_string()
            .contains("absolute"));
        assert!(WorkspacePath::new(r"a\b")
            .unwrap_err()
            .to_string()
            .contains("backslashes"));
    }
    #[test]
    fn normalizes_relative_current_directory() {
        assert_eq!(WorkspacePath::new("./a//b").unwrap().as_str(), "a/b");
        assert_eq!(WorkspacePath::new(".").unwrap(), WorkspacePath::root());
    }
}
