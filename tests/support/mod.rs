//! Shared native workload fixture; no test-only command is added to production code.
use std::env;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use tempfile::TempDir;

/// Compile the fixture once per integration-test executable and retain its directory.
/// Commands accept ordinary argument vectors; see `workload.rs` for their behavior.
pub fn workload_program() -> &'static str {
    static FIXTURE: OnceLock<(TempDir, PathBuf)> = OnceLock::new();
    let (_, program) = FIXTURE.get_or_init(|| {
        let directory = tempfile::tempdir().unwrap();
        let program = directory.path().join("workspace-test-workload");
        let compiled = Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .arg("--edition=2021")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/support/workload.rs"
            ))
            .arg("-o")
            .arg(&program)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        (directory, program)
    });
    program.to_str().unwrap()
}
