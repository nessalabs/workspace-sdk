#!/bin/sh
# Mechanical half of the gate. Independent agent reviews are also mandatory.
set -eu
script_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$script_directory/.."
cargo fmt --all -- --check
cargo clippy --locked --all-features --all-targets -- -D warnings
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --no-default-features --all-targets -- -D warnings
cargo clippy --locked --no-default-features --features portability --all-targets -- -D warnings
cargo clippy --locked --no-default-features --features container --all-targets -- -D warnings
cargo build --locked --all-features --example pty_live_probe
cargo test --locked --all-features
cargo test --locked --no-default-features --features space --test space_provider_contract
cargo build --locked --no-default-features --example pty_live_probe
cargo test --locked --no-default-features
cargo test --locked
cargo test --locked --no-default-features --features portability
RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps
RUSTDOCFLAGS='-D warnings' cargo doc --locked --all-features --no-deps
cargo build --locked --bin workspace-supervisor
cargo build --locked --no-default-features --features portability --bin workspace-supervisor
cargo test --locked --test runtime_performance -- --ignored --nocapture
cargo test --locked --test runtime_pty_performance -- --ignored --nocapture
printf '%s\n' 'Mechanical checks passed. Record independent design and adversarial review decisions before accepting the quality gate.'
