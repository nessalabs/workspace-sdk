#!/bin/sh
# Mechanical half of the gate. Independent agent reviews are also mandatory.
set -eu
script_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$script_directory/.."
cargo fmt --all -- --check
cargo clippy --locked --all-features --all-targets -- -D warnings
cargo test --locked --all-features
cargo test --locked --no-default-features
RUSTDOCFLAGS='-D warnings' cargo doc --locked --all-features --no-deps
cargo build --locked --bin workspace-supervisor
cargo test --locked --test runtime_performance -- --ignored --nocapture
cargo test --locked --test runtime_pty_performance -- --ignored --nocapture
printf '%s\n' 'Mechanical checks passed. Record independent design and adversarial review decisions before accepting the quality gate.'
