#!/bin/sh
# Run what CI runs, before pushing: formatting, lints, tests, the fuzz targets' build,
# cargo deny, and the build with the oldest supported Rust.
#
#   scripts/check.sh           everything
#   scripts/check.sh --quick   formatting, lints and tests only
set -eu
cd "$(dirname "$0")/.."

step() { printf '\n\033[1m== %s\033[0m\n' "$1"; }

step "Formatting"
cargo fmt --check

step "Lints"
cargo clippy --locked --all-targets -- -D warnings

step "Tests"
cargo test --locked

if [ "${1:-}" = "--quick" ]; then
    printf '\nQuick checks passed.\n'
    exit 0
fi

step "Fuzz targets build"
cargo check --locked --manifest-path fuzz/Cargo.toml

step "Dependencies (cargo deny)"
if command -v cargo-deny >/dev/null 2>&1; then
    cargo deny --log-level error --locked check
else
    echo "skipped: install it with  cargo install cargo-deny --locked"
fi

# The toolchain's own cargo and rustc, by path: a cargo from elsewhere on PATH (Homebrew, say)
# would otherwise build with itself and the check would mean nothing
msrv=$(sed -n 's/^rust-version = "\(.*\)"/\1/p' Cargo.toml)
step "Oldest supported Rust ($msrv)"
if toolchain_cargo=$(rustup which --toolchain "$msrv" cargo 2>/dev/null); then
    bin=$(dirname "$toolchain_cargo")
    PATH="$bin:$PATH" CARGO_TARGET_DIR=target/msrv "$bin/cargo" check --locked --all-targets
else
    echo "skipped: install it with  rustup toolchain install $msrv --profile minimal"
fi

printf '\nAll checks passed.\n'
