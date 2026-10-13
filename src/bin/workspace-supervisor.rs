//! Single-threaded target process; fork happens before workload I/O creates threads.
use std::process::exit;
use workspace_sdk::supervisor::run_cli;
fn main() {
    // This dedicated executable has not created any threads.
    exit(unsafe { run_cli() });
}
