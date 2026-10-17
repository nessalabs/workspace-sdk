use super::{GitWorktree, GitWorktreeHead};
use crate::{Error, Result};
use std::collections::HashSet;

pub(super) const MAX_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_WORKTREES: usize = 256;
const MAX_PATH_BYTES: usize = 16 * 1024;
const MAX_REASON_BYTES: usize = 4 * 1024;
const MAX_REFERENCE_BYTES: usize = 4 * 1024;

fn invalid() -> Error {
    Error::State("Git worktree discovery returned invalid or unsupported porcelain".into())
}

pub(super) fn parse(output: &str) -> Result<Vec<GitWorktree>> {
    if output.len() > MAX_OUTPUT_BYTES || output.contains('\u{fffd}') {
        return Err(invalid());
    }
    let records = output.strip_suffix("\0\0").ok_or_else(invalid)?;
    let mut worktrees = Vec::new();
    let mut paths = HashSet::new();
    for record in records.split("\0\0") {
        if worktrees.len() == MAX_WORKTREES {
            return Err(invalid());
        }
        let worktree = parse_record(record)?;
        if !paths.insert(worktree.path.clone()) {
            return Err(invalid());
        }
        worktrees.push(worktree);
    }
    Ok(worktrees)
}

fn parse_record(record: &str) -> Result<GitWorktree> {
    let mut fields = record.split('\0');
    let path = fields
        .next()
        .and_then(|field| field.strip_prefix("worktree "))
        .filter(|path| !path.is_empty() && path.len() <= MAX_PATH_BYTES)
        .ok_or_else(invalid)?;
    let mut seen = HashSet::new();
    let mut oid = None;
    let mut branch = None;
    let mut bare = false;
    let mut detached = false;
    let mut locked = None;
    let mut prunable = None;
    for field in fields {
        let (name, value) = field
            .split_once(' ')
            .map_or((field, None), |(key, value)| (key, Some(value)));
        if !seen.insert(name) {
            return Err(invalid());
        }
        match (name, value) {
            ("HEAD", Some(value)) if valid_oid(value) => oid = Some(value),
            ("branch", Some(value)) if valid_branch(value) => branch = Some(value),
            ("bare", None) => bare = true,
            ("detached", None) => detached = true,
            ("locked", reason) if reason.unwrap_or("").len() <= MAX_REASON_BYTES => {
                locked = Some(reason.unwrap_or("").to_owned())
            }
            ("prunable", reason) if reason.unwrap_or("").len() <= MAX_REASON_BYTES => {
                prunable = Some(reason.unwrap_or("").to_owned())
            }
            _ => return Err(invalid()),
        }
    }
    let head = match (bare, oid, branch, detached) {
        (true, None, None, false) => GitWorktreeHead::Bare,
        (false, Some(oid), Some(reference), false) => GitWorktreeHead::Branch {
            reference: reference.to_owned(),
            oid: oid.to_owned(),
        },
        (false, Some(oid), None, true) if oid.bytes().any(|byte| byte != b'0') => {
            GitWorktreeHead::Detached {
                oid: oid.to_owned(),
            }
        }
        _ => return Err(invalid()),
    };
    Ok(GitWorktree {
        path: path.to_owned(),
        head,
        locked,
        prunable,
    })
}

fn valid_oid(oid: &str) -> bool {
    matches!(oid.len(), 40 | 64) && oid.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_branch(reference: &str) -> bool {
    reference.len() <= MAX_REFERENCE_BYTES
        && reference.starts_with("refs/heads/")
        && !reference.ends_with('.')
        && !reference.contains("..")
        && !reference.contains("@{")
        && !reference
            .bytes()
            .any(|byte| byte <= b' ' || byte == 127 || b"~^:?*[\\".contains(&byte))
        && reference
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OID: &str = "0123456789012345678901234567890123456789";

    fn branch(path: &str) -> String {
        format!("worktree {path}\0HEAD {OID}\0branch refs/heads/main\0\0")
    }

    #[test]
    fn preserves_paths_and_empty_or_multiline_reasons() {
        let data = format!("worktree /a space\nand newline\0HEAD {OID}\0detached\0locked\0prunable a reason\nwith spaces\0\0");
        let parsed = parse(&data).unwrap();
        assert_eq!(parsed[0].path, "/a space\nand newline");
        assert_eq!(parsed[0].locked.as_deref(), Some(""));
        assert_eq!(parsed[0].prunable.as_deref(), Some("a reason\nwith spaces"));
        assert_eq!(
            parsed[0].head,
            GitWorktreeHead::Detached { oid: OID.into() }
        );
    }

    #[test]
    fn accepts_bare_unborn_sha256_and_multiple_records() {
        let data = format!(
            "worktree /bare\0bare\0\0{}worktree /sha256\0HEAD {}\0detached\0\0",
            branch("/unborn").replace(OID, &"0".repeat(40)),
            "a".repeat(64)
        );
        let parsed = parse(&data).unwrap();
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].head, GitWorktreeHead::Bare);
        assert_eq!(parsed[0].locked, None);
        assert_eq!(parsed[0].prunable, None);
        assert!(
            matches!(&parsed[1].head, GitWorktreeHead::Branch { oid, .. } if oid == &"0".repeat(40))
        );
    }

    #[test]
    fn rejects_partial_unknown_duplicate_and_inconsistent_records() {
        let good = branch("/repo");
        let cases = vec![
            String::new(),
            good.trim_end_matches('\0').into(),
            format!("{}\0", good.trim_end_matches('\0')),
            format!("{good}\0"),
            format!("{good}{good}"),
            good.replace("worktree /repo", "worktree "),
            good.replace("/repo", "/re\u{fffd}po"),
            good.replace("HEAD ", "unknown "),
            good.replace(OID, "123"),
            good.replace(OID, &"z".repeat(40)),
            good.replace("branch refs/heads/main", "branch refs/tags/main"),
            good.replace("branch refs/heads/main", "branch refs/heads/a..b"),
            good.replace("branch refs/heads/main", "branch refs/heads/a.lock"),
            good.replace("branch refs/heads/main", "branch refs/heads/main\0detached"),
            good.replace("branch refs/heads/main", "bare"),
            good.replace("branch refs/heads/main\0", ""),
            good.replace("branch refs/heads/main", "branch refs/heads/main\0HEAD a"),
            good.replace(
                "branch refs/heads/main",
                "branch refs/heads/main\0locked a\0locked b",
            ),
            "worktree /bare\0bare\0bare\0\0".into(),
            "worktree /bare\0bare value\0\0".into(),
            "worktree /bare\0bare\0detached\0\0".into(),
            good.replace("branch refs/heads/main", "detached value"),
            good.replace("branch refs/heads/main", "detached")
                .replace(OID, &"0".repeat(40)),
            good.replace("branch refs/heads/main", "branch refs/heads/"),
            good.replace("branch refs/heads/main", "branch refs/heads/a b"),
            good.replace(
                "branch refs/heads/main",
                "branch refs/heads/main\0worktree /other",
            ),
            good.replace(
                "branch refs/heads/main",
                "branch refs/heads/main\0prunable a\0prunable b",
            ),
            "HEAD 123\0worktree /repo\0bare\0\0".into(),
        ];
        for case in cases {
            assert!(parse(&case).is_err(), "accepted {case:?}");
        }
    }

    #[test]
    fn enforces_output_record_path_reason_and_reference_bounds() {
        assert!(parse(&"x".repeat(MAX_OUTPUT_BYTES + 1)).is_err());
        let records: String = (0..=MAX_WORKTREES)
            .map(|i| branch(&format!("/r{i}")))
            .collect();
        assert!(parse(&records).is_err());
        assert!(parse(&branch(&"p".repeat(MAX_PATH_BYTES + 1))).is_err());
        assert!(parse(&branch("/r").replace(
            "branch refs/heads/main",
            &format!(
                "branch refs/heads/main\0locked {}",
                "x".repeat(MAX_REASON_BYTES + 1)
            )
        ))
        .is_err());
        assert!(parse(&branch("/r").replace(
            "refs/heads/main",
            &format!("refs/heads/{}", "x".repeat(MAX_REFERENCE_BYTES))
        ))
        .is_err());
        assert!(parse(&branch(&"p".repeat(MAX_PATH_BYTES))).is_ok());
        let at_limit: String = (0..MAX_WORKTREES)
            .map(|i| branch(&format!("/r{i}")))
            .collect();
        assert_eq!(parse(&at_limit).unwrap().len(), MAX_WORKTREES);
    }

    #[test]
    fn accepts_exact_metadata_and_total_output_limits() {
        let reason = "x".repeat(MAX_REASON_BYTES);
        let reference = format!(
            "refs/heads/{}",
            "x".repeat(MAX_REFERENCE_BYTES - "refs/heads/".len())
        );
        let record = branch("/r")
            .replace("refs/heads/main", &reference)
            .replace("\0\0", &format!("\0locked {reason}\0prunable {reason}\0\0"));
        assert!(parse(&record).is_ok());
        let mut output: String = (0..15)
            .map(|i| branch(&format!("/{i}{}", "p".repeat(MAX_PATH_BYTES - 3))))
            .collect();
        let remaining = MAX_OUTPUT_BYTES - output.len();
        let overhead = branch("").len();
        output.push_str(&branch(&format!(
            "/{}",
            "p".repeat(remaining - overhead - 1)
        )));
        assert_eq!(output.len(), MAX_OUTPUT_BYTES);
        assert!(parse(&output).is_ok());
        output.insert(10, 'x');
        assert!(parse(&output).is_err());
    }
}
