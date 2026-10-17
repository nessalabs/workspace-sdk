/// Target-host POSIX syntax only; never interprets paths on the controller.
pub(super) fn valid_worktree_path(path: &str, allow_root: bool) -> bool {
    path.len() <= 16 * 1024
        && path.starts_with('/')
        && !path.contains(['\0', '\u{fffd}'])
        && ((allow_root && path == "/")
            || !path
                .split('/')
                .skip(1)
                .any(|part| matches!(part, "" | "." | "..")))
}

pub(super) fn valid_launch_bounds(timeout_seconds: u64, output_limit_bytes: u64) -> bool {
    (1..=3600).contains(&timeout_seconds) && (2..=16 * 1024 * 1024).contains(&output_limit_bytes)
}
